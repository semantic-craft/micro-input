import hashlib
import importlib.machinery
import importlib.util
from pathlib import Path
import unittest


loader = importlib.machinery.SourceFileLoader(
    "check_boundaries",
    str(Path(__file__).parents[1] / "check-boundaries.py"),
)
spec = importlib.util.spec_from_loader(loader.name, loader)
module = importlib.util.module_from_spec(spec)
loader.exec_module(module)


class CheckBoundariesTests(unittest.TestCase):
    def test_clean_room_scan_rejects_leaks_and_allows_legitimate_docs_and_fixtures(self):
        self.assertTrue(module.find_clean_room_leaks("docs/note.md", "see .scratch/input-study/foo"))
        self.assertTrue(module.find_clean_room_leaks("docs/note.md", "see .scratch/input-study"))
        self.assertTrue(module.find_clean_room_leaks("crates/mi-core/src/a.rs", ".scratch/input-study/foo"))
        self.assertTrue(module.find_clean_room_leaks("crates/mi-core/src/a.rs", "see .scratch/input-study"))
        self.assertTrue(module.find_clean_room_leaks("scripts/a.py", "ROOT / '.scratch/input-study'"))
        self.assertTrue(module.find_clean_room_leaks("docs/note.md", "/Applications/input.app"))
        self.assertTrue(module.find_clean_room_leaks("docs/note.md", "app." + "asar"))
        self.assertTrue(module.find_clean_room_leaks("docs/note.md", "dist-" + "electron/main/index.js"))
        self.assertTrue(module.find_clean_room_leaks("docs/note.md", "K:13730"))
        self.assertTrue(module.find_clean_room_leaks("docs/note.md", "ev_" + "0" * 64))
        self.assertTrue(module.find_clean_room_leaks("docs/note.md", "0x420742ac"))
        self.assertTrue(module.find_clean_room_leaks("docs/note.md", "wl_host_follow." + "cpp"))
        self.assertTrue(module.find_clean_room_leaks("docs/note.md", "wl_lights." + "cpp"))
        self.assertTrue(module.find_clean_room_leaks("crates/mi-core/src/a.rs", "wl_lights." + "cpp"))
        self.assertTrue(module.find_clean_room_leaks("crates/mi-core/src/a.rs", "/dev/cu.usbmodem" + "11401"))
        self.assertTrue(module.find_clean_room_leaks("docs/note.md", "de:ad:" + "be:ef:ca:fe"))
        self.assertTrue(module.find_clean_room_leaks("crates/mi-core/src/a.rs", "de-ad-" + "be-ef-ca-fe"))
        self.assertTrue(module.find_clean_room_leaks("docs/note.md", "mac:de:ad:" + "be:ef:ca:fe"))
        self.assertTrue(module.find_clean_room_leaks("crates/mi-core/src/a.rs", "usb-de-ad-" + "be-ef-ca-fe"))
        self.assertTrue(module.find_clean_room_leaks("docs/note.md", "de:ad-" + "be:ef-ca:fe"))
        self.assertTrue(module.find_clean_room_leaks("docs/note.md", "firmware-1794634846067366000-95189"))
        self.assertTrue(module.find_clean_room_leaks("docs/note.md", "xw-notary"))
        self.assertTrue(module.find_clean_room_leaks("scripts/release.sh", "notarytool --keychain-profile xw-notary"))
        self.assertTrue(module.find_clean_room_leaks("scripts/release-macos", "micro-input-notary"))
        self.assertTrue(module.find_clean_room_leaks("docs/note.md", "custom-app-notary"))
        self.assertEqual(
            module.find_clean_room_leaks("scripts/release-macos", "macos-notary-profile"),
            [],
        )
        self.assertEqual(
            module.find_clean_room_leaks("scripts/release-macos", "macos-notary submit"),
            [],
        )
        self.assertTrue(module.find_clean_room_leaks("docs/note.md", "input-app-lean-triage"))
        self.assertTrue(module.find_clean_room_leaks("crates/mi-core/src/a.rs", "input-app-lean-triage"))

        synthetic_sha1 = "deadbeef" * 5
        synthetic_sha1_digest = hashlib.sha256(b"mi-cleanroom-v1:" + synthetic_sha1.encode("ascii")).hexdigest()
        synthetic_sha256 = "cafebabe12345678" + "0" * 48
        synthetic_sha256_digest = hashlib.sha256(b"mi-cleanroom-v1:" + synthetic_sha256[:16].encode("ascii")).hexdigest()
        self.assertTrue(
            module.find_clean_room_leaks(
                "docs/note.md",
                synthetic_sha1.upper(),
                extra_sha1_digests=[synthetic_sha1_digest],
            )
        )
        self.assertTrue(
            module.find_clean_room_leaks(
                "crates/mi-core/src/a.rs",
                synthetic_sha256.upper(),
                extra_sha256_prefix_digests=[synthetic_sha256_digest],
            )
        )
        self.assertTrue(
            module.find_clean_room_leaks(
                "crates/mi-core/src/a.rs",
                "0x" + synthetic_sha256,
                extra_sha256_prefix_digests=[synthetic_sha256_digest],
            )
        )
        self.assertTrue(
            module.find_clean_room_leaks(
                "docs/note.md",
                "0x" + synthetic_sha1,
                extra_sha1_digests=[synthetic_sha1_digest],
            )
        )

        self.assertEqual(
            module.find_clean_room_leaks(
                "crates/mi-session/tests/support/mod.rs",
                'let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.scratch/hardware-probes");',
            ),
            [],
        )
        self.assertEqual(
            module.find_clean_room_leaks(
                "README.md",
                "Accurate main-session filtering; /dev/cu.usbmodem-test-101; sha256:1d8bd545b9047182d7112dd86b14f6c636bad3834725ee0a6eca7cacee0ec6a7",
            ),
            [],
        )
        self.assertEqual(
            module.find_clean_room_leaks(
                "crates/mi-macos/src/scenarios.rs",
                "macs 02:00:5e:10:00:01 02:00-5e:10-00:01 aa:bb:cc:dd:ee:ff at 12:34:56 on 2026-11-16 uuid 12345678-1234-1234-1234-123456789012 oid 00:11:22:33:44:55:66",
            ),
            [],
        )
        self.assertEqual(
            module.find_clean_room_leaks(
                "crates/mi-proto/src/lib.rs",
                r'out.push_str(&format!("\\u{u:04x}"));',
            ),
            [],
        )
        self.assertEqual(
            module.find_clean_room_leaks(
                "skills/verify-micro-input/SKILL.md",
                ".scratch/qa-runs/<timestamp>-<pid>/",
            ),
            [],
        )

    def test_non_test_unix_stream_connect_is_confined_to_bridge_ipc(self):
        sample = """
fn prod() {
    let _ = UnixStream::connect("/tmp/a.sock");
}
#[cfg(test)]
mod tests {
    fn test_only() {
        let _ = UnixStream::connect("/tmp/b.sock");
    }
}
"""
        self.assertTrue(module.find_forbidden_unix_connect("crates/mi-core/src/store.rs", sample))
        self.assertEqual(module.find_forbidden_unix_connect("crates/mi-bridge/src/ipc.rs", sample), [])

        test_only_sample = """
fn prod() {}
#[cfg(test)]
mod tests {
    fn test_only() {
        let _ = UnixStream::connect("/tmp/b.sock");
    }
}
"""
        self.assertEqual(module.find_forbidden_unix_connect("crates/mi-core/src/store.rs", test_only_sample), [])
        self.assertEqual(
            module.find_forbidden_unix_connect(
                "crates/mi-core/src/store_tests.rs",
                'fn t() { let _ = UnixStream::connect("/tmp/b.sock"); }',
            ),
            [],
        )

        mid_file_test_helper = """
#[cfg(test)]
fn helper_with_braces() {
    let _ = "{ not unmatched }";
}

fn real_prod_after_helper() {
    let _ = UnixStream::connect("/tmp/prod.sock");
}
"""
        self.assertTrue(
            module.find_forbidden_unix_connect("crates/mi-core/src/session_navigation.rs", mid_file_test_helper)
        )

    def test_ffi_extern_c_entrypoints_require_ffi_guard(self):
        bad_ffi = """
#[no_mangle]
pub extern "C" fn mi_core_bad() -> i32 {
    let _brace = "{";
    0
}
"""
        good_ffi = """
#[no_mangle]
pub unsafe extern "C" fn mi_core_good(ptr: *mut c_char) -> i32 {
    ffi_guard(-1, "mi_core_good", || {
        0
    })
}
"""
        self.assertEqual(module.find_unguarded_ffi_entrypoints("crates/mi-ffi/src/lib.rs", bad_ffi), ["mi_core_bad"])
        self.assertEqual(module.find_unguarded_ffi_entrypoints("crates/mi-ffi/src/lib.rs", good_ffi), [])

    def test_rust_lexical_and_cfg_test_scanners_handle_edge_cases(self):
        cfg_variants = """
#[cfg(all(test, unix))]
mod unix_tests {
    fn t() { let _ = UnixStream::connect("/tmp/t1.sock"); }
}
#[cfg(any(test, feature = "sim"))]
fn sim_helper() {
    let _ = UnixStream::connect("/tmp/t2.sock");
}
"""
        self.assertEqual(module.find_forbidden_unix_connect("crates/mi-core/src/a.rs", cfg_variants), [])

        not_test_prod = """
#[cfg(not(test))]
fn prod_only() {
    let _ = UnixStream::connect("/tmp/prod.sock");
}
"""
        self.assertTrue(module.find_forbidden_unix_connect("crates/mi-core/src/a.rs", not_test_prod))

        literals_and_comments = r'''
fn safe<'a>(x: &'a str) {
    // UnixStream::connect("/tmp/comment.sock");
    /* outer /* nested */ UnixStream::connect("/tmp/nested.sock"); */
    let _ = "UnixStream::connect(\"/tmp/str.sock\")";
    let _ = b"UnixStream::connect(\"/tmp/bstr.sock\")";
    let _ = c"UnixStream::connect(\"/tmp/cstr.sock\")";
    let _ = r##"UnixStream::connect("#")"##;
    let _ = br#"UnixStream::connect("x")"#;
    let _ = '\'';
}
'''
        self.assertEqual(module.find_forbidden_unix_connect("crates/mi-core/src/a.rs", literals_and_comments), [])

        lifetime_then_call = """
fn prod<'a, 'b>(_x: &'a str, _y: &'b str) {
    let _ = UnixStream :: connect ("/tmp/ws.sock");
}
"""
        self.assertTrue(module.find_forbidden_unix_connect("crates/mi-core/src/a.rs", lifetime_then_call))

        ffi_edge_cases = r'''
// #[no_mangle] pub extern "C" fn commented_out() {}
#[unsafe(no_mangle)]
pub extern "C" fn spoofed_str<'a>(_: &'a u8) {
    let _ = "ffi_guard(";
    // ffi_guard(
    let _brace = "{";
}

#[no_mangle]
#[inline(never)]
pub unsafe extern "C" fn multi_attr_unguarded() -> i32 {
    0
}

#[unsafe(no_mangle)]
#[inline(never)]
pub unsafe extern "C" fn guarded_ok<'a>(_x: &'a u8) -> i32 {
    let _c = '\'';
    ffi_guard(-1, "guarded_ok", || 0)
}
'''
        self.assertEqual(
            module.find_unguarded_ffi_entrypoints("crates/mi-ffi/src/lib.rs", ffi_edge_cases),
            ["spoofed_str", "multi_attr_unguarded"],
        )
