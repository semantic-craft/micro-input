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
        self.assertTrue(module.find_clean_room_leaks("docs/note.md", "/Applications/input.app"))
        self.assertTrue(module.find_clean_room_leaks("docs/note.md", "app." + "asar"))
        self.assertTrue(module.find_clean_room_leaks("docs/note.md", "dist-" + "electron/main/index.js"))
        self.assertTrue(module.find_clean_room_leaks("docs/note.md", "K:13730"))
        self.assertTrue(module.find_clean_room_leaks("docs/note.md", "ev_" + "0" * 64))
        self.assertTrue(module.find_clean_room_leaks("docs/note.md", "0x420742ac"))
        self.assertTrue(module.find_clean_room_leaks("docs/note.md", "wl_host_follow." + "cpp"))
        self.assertTrue(module.find_clean_room_leaks("crates/mi-core/src/a.rs", "/dev/cu.usbmodem" + "11401"))
        self.assertTrue(module.find_clean_room_leaks("docs/note.md", "30:" + "ED:A0:1D:9E:30"))
        self.assertTrue(module.find_clean_room_leaks("docs/note.md", "firmware-1794634846067366000-95189"))
        self.assertTrue(module.find_clean_room_leaks("docs/note.md", "a6c7185d" + "5c3d8dda" + "0" * 48))

        self.assertEqual(
            module.find_clean_room_leaks(
                "crates/mi-session/tests/support/mod.rs",
                'let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.scratch/input-study");',
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
