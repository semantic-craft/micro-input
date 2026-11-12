#!/usr/bin/env python3
"""Sign macOS .app bundles inside-out and manage LaunchServices registration."""
from __future__ import annotations

import argparse
import os
from pathlib import Path
import re
import subprocess
import sys

MACHO_MAGICS = {
    b'\xfe\xed\xfa\xce',
    b'\xce\xfa\xed\xfe',
    b'\xfe\xed\xfa\xcf',
    b'\xcf\xfa\xed\xfe',
    b'\xca\xfe\xba\xbe',
    b'\xbe\xba\xfe\xca',
    b'\xca\xfe\xba\xbf',
    b'\xbf\xba\xfe\xca',
}

LSREGISTER = Path(
    '/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister'
)


def is_macho_file(path: Path) -> bool:
    if path.is_symlink() or not path.is_file():
        return False
    try:
        with path.open('rb') as handle:
            magic = handle.read(4)
    except OSError:
        return False
    return magic in MACHO_MAGICS


def validate_framework_symlinks(bundle_path: Path) -> None:
    frameworks_dir = bundle_path / 'Contents/Frameworks'
    if not frameworks_dir.is_dir():
        return
    for entry in sorted(frameworks_dir.iterdir()):
        if entry.suffix != '.framework':
            continue
        if entry.is_symlink():
            raise ValueError(f'Framework bundle must be a directory, got symlink: {entry.name}')
        versions_dir = entry / 'Versions'
        if not versions_dir.exists():
            continue
        current_link = versions_dir / 'Current'
        if not current_link.is_symlink():
            raise ValueError(
                f'{entry.name}/Versions/Current must remain a symbolic link (copy frameworks with ditto or symlinks=True)'
            )
        target = Path(os.readlink(current_link))
        resolved_target = (versions_dir / target)
        if not resolved_target.exists():
            raise ValueError(f'{entry.name}/Versions/Current points to missing target: {target}')
        for top_item in sorted(entry.iterdir()):
            if top_item.name in ('Versions', '_CodeSignature'):
                continue
            if not top_item.is_symlink():
                raise ValueError(
                    f'{entry.name}/{top_item.name} must be a symbolic link into Versions/Current, not a materialized copy'
                )


def _is_bundle_main_executable(file_path: Path) -> bool:
    parent = file_path.parent
    if parent.name == 'MacOS' and parent.parent.name == 'Contents':
        enclosing = parent.parent.parent
        if enclosing.suffix in ('.app', '.xpc'):
            return True
    if parent.parent.name == 'Versions':
        framework_dir = parent.parent.parent
        if framework_dir.suffix == '.framework' and framework_dir.stem == file_path.name:
            return True
    if parent.suffix == '.framework' and parent.stem == file_path.name:
        return True
    return False


def collect_sign_targets(bundle_path: Path) -> list[Path]:
    bundle = bundle_path.resolve() if not bundle_path.is_symlink() else bundle_path
    if not (bundle / 'Contents/Info.plist').is_file():
        raise ValueError(f'Missing Contents/Info.plist in bundle: {bundle.name}')
    validate_framework_symlinks(bundle)

    nested: list[Path] = []
    contents_dir = bundle / 'Contents'
    for root_str, dirnames, filenames in os.walk(contents_dir, followlinks=False):
        root_dir = Path(root_str)
        kept_dirs: list[str] = []
        for dname in sorted(dirnames):
            candidate = root_dir / dname
            if candidate.is_symlink():
                continue
            if candidate.suffix in ('.xpc', '.app', '.framework'):
                nested.append(candidate)
            kept_dirs.append(dname)
        dirnames[:] = kept_dirs

        for fname in sorted(filenames):
            candidate = root_dir / fname
            if candidate.is_symlink():
                continue
            if candidate.suffix in ('.dylib', '.so'):
                nested.append(candidate)
                continue
            try:
                rel_to_contents = candidate.relative_to(contents_dir)
            except ValueError:
                continue
            if rel_to_contents.parts and rel_to_contents.parts[0] == 'Frameworks':
                if not _is_bundle_main_executable(candidate) and is_macho_file(candidate):
                    nested.append(candidate)

    nested.sort(key=lambda p: (-len(p.relative_to(bundle).parts), str(p.relative_to(bundle))))
    nested.append(bundle)
    return nested


def _discover_dev_identity() -> str:
    env_id = os.environ.get('MI_CODESIGN_IDENTITY', '').strip()
    if env_id:
        return env_id
    try:
        out = subprocess.check_output(
            ['security', 'find-identity', '-v', '-p', 'codesigning'],
            text=True,
            stderr=subprocess.DEVNULL,
        )
    except (OSError, subprocess.CalledProcessError):
        return '-'
    match = re.search(r'"(Apple Development:[^"]+)"', out)
    return match.group(1) if match else '-'


def _redact_identity(message: str, identity: str) -> str:
    redacted = message
    if identity and identity != '-':
        redacted = redacted.replace(identity, '<REDACTED_SIGNING_IDENTITY>')
    redacted = re.sub(r'\b[0-9A-F]{40}\b', '<REDACTED_SHA1>', redacted)
    redacted = re.sub(r'\([A-Z0-9]{10}\)', '(<REDACTED_TEAM>)', redacted)
    return redacted


def sign_bundle(bundle_path: Path, mode: str = 'dev', identity: str | None = None) -> list[Path]:
    if mode not in ('dev', 'release'):
        raise ValueError(f'Unsupported signing mode: {mode}')
    resolved_identity = (identity or '').strip()
    if not resolved_identity:
        if mode == 'release':
            raise ValueError('Release signing requires an explicit Developer ID Application identity')
        resolved_identity = _discover_dev_identity()
    if mode == 'release' and resolved_identity == '-':
        raise ValueError('Release signing forbids ad-hoc (-) identity')

    targets = collect_sign_targets(bundle_path)
    for target in targets:
        cmd = ['codesign', '--force']
        if mode == 'release':
            cmd.extend(['--options', 'runtime', '--timestamp'])
        else:
            cmd.append('--timestamp=none')
        cmd.extend(['--sign', resolved_identity, str(target)])
        proc = subprocess.run(cmd, capture_output=True, text=True)
        if proc.returncode != 0:
            err = _redact_identity((proc.stderr or proc.stdout or '').strip(), resolved_identity)
            raise RuntimeError(f'codesign failed for {target.name}: {err}')
    return targets


def collect_nested_apps(bundle_path: Path) -> list[Path]:
    apps: list[Path] = []
    if not bundle_path.exists():
        return apps
    frameworks_dir = bundle_path / 'Contents/Frameworks'
    if frameworks_dir.is_dir():
        for root_str, dirnames, _filenames in os.walk(frameworks_dir, followlinks=False):
            root_dir = Path(root_str)
            kept: list[str] = []
            for dname in sorted(dirnames):
                candidate = root_dir / dname
                if candidate.suffix == '.app':
                    apps.append(candidate)
                if not candidate.is_symlink():
                    kept.append(dname)
            dirnames[:] = kept
    apps.append(bundle_path)
    return apps


def unregister_launch_services(bundle_path: Path, lsregister: Path = LSREGISTER) -> None:
    if not lsregister.is_file():
        return
    for candidate in collect_nested_apps(bundle_path):
        subprocess.run(
            [str(lsregister), '-u', str(candidate)],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            check=False,
        )


def _parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(prog='scripts/sign_bundle.py', description=__doc__, allow_abbrev=False)
    parser.add_argument('bundle', type=Path, help='Path to outer .app bundle')
    parser.add_argument('--mode', choices=('dev', 'release'), default='dev')
    parser.add_argument('--identity', default=None, help='Explicit codesign identity')
    parser.add_argument('--unregister-ls', action='store_true', help='Unregister outer and nested .app bundles from LaunchServices')
    parser.add_argument('--unregister-only', action='store_true', help='Only unregister from LaunchServices without signing')
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = _parse_args(argv if argv is not None else sys.argv[1:])
    bundle = args.bundle.resolve()
    if args.unregister_only:
        unregister_launch_services(bundle)
        return 0
    sign_bundle(bundle, mode=args.mode, identity=args.identity)
    if args.unregister_ls:
        unregister_launch_services(bundle)
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
