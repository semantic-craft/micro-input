#!/usr/bin/env python3
"""Local subprocess example: read -> validate -> independent proposal. Never accepts it."""
import json
import pathlib
import subprocess
import sys


def call(executable, command, request):
    process = subprocess.run([str(executable), 'draft', command], input=json.dumps(request),
                             text=True, capture_output=True, check=False)
    envelope = json.loads(process.stdout)
    if process.returncode or not envelope['ok']:
        raise RuntimeError(json.dumps(envelope.get('error', {}), ensure_ascii=False))
    return envelope['result']


def main():
    if len(sys.argv) not in (3, 4):
        raise SystemExit('Usage: scripts/draft-client.py WORKBENCH OUTPUT [NAME]')
    root = pathlib.Path(__file__).resolve().parent.parent
    executable = root / 'target/debug/mi'
    common = {'api_version': 1, 'workbench': str(pathlib.Path(sys.argv[1]).resolve())}
    current = call(executable, 'read', common)
    profile = current['document']['graph']['profile_order'][0]
    request = {**common, 'expected': current['stamp'], 'operations': [
        {'op': 'rename_profile', 'profile_id': profile,
         'name': sys.argv[3] if len(sys.argv) == 4 else 'Agent 草稿示例'}]}
    call(executable, 'validate', request)
    result = call(executable, 'patch', {**request, 'output': str(pathlib.Path(sys.argv[2]).resolve())})
    print(json.dumps({'proposal_path': result['proposal_path'], 'changes': result['changes'],
                      'next': '在工作台“审阅 Agent 提案…”中选择此文件'}, ensure_ascii=False))


if __name__ == '__main__':
    main()
