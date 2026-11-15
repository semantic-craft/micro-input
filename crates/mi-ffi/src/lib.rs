use mi_core::{
    qa_runner::{run_synthetic_scenario, QaLaunchEnv},
    WorkbenchCommand, WorkbenchStore, WorkbenchStoreConfig,
};
use mi_macos::DocumentInstanceLock;
use std::{
    ffi::{c_char, CStr, CString},
    path::PathBuf,
    ptr,
};

fn ffi_safe_log(prefix: &str, detail: &str) {
    let mut stderr = std::io::stderr();
    let _ = std::io::Write::write_all(&mut stderr, prefix.as_bytes());
    let _ = std::io::Write::write_all(&mut stderr, detail.as_bytes());
    let _ = std::io::Write::write_all(&mut stderr, b"\n");
}

fn ffi_guard<T>(fallback: T, name: &str, f: impl FnOnce() -> T) -> T {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(val) => val,
        Err(payload) => {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(payload)));
            ffi_safe_log("micro-input FFI panic in ", name);
            fallback
        }
    }
}

unsafe fn parse_optional_trimmed_path(raw_ptr: *const c_char) -> Result<Option<PathBuf>, ()> {
    if raw_ptr.is_null() {
        return Ok(None);
    }
    let raw = CStr::from_ptr(raw_ptr).to_str().map_err(|_| ())?;
    let trimmed = raw.trim();
    Ok((!trimmed.is_empty()).then(|| PathBuf::from(trimmed)))
}

fn string_to_c_char(s: String) -> *mut c_char {
    match CString::new(s) {
        Ok(cstr) => cstr.into_raw(),
        Err(_) => ptr::null_mut(),
    }
}

#[no_mangle]
pub extern "C" fn mi_core_run_qa_from_env() -> i32 {
    ffi_guard(-1, "mi_core_run_qa_from_env", || {
        let launch_env = match QaLaunchEnv::from_env() {
            Ok(env) => env,
            Err(err) => {
                eprintln!("Micro Input launch env error: {err}");
                return 1;
            }
        };
        let Some(scenario) = launch_env.scenario else {
            return -1;
        };
        let _instance_lock = match DocumentInstanceLock::acquire(&launch_env.document_dir) {
            Ok(Some(lock)) => lock,
            Ok(None) => {
                eprintln!(
                    "Micro Input already running for {}",
                    launch_env.document_dir.display()
                );
                return 1;
            }
            Err(err) => {
                eprintln!("Micro Input instance lock failed: {err}");
                return 1;
            }
        };
        match run_synthetic_scenario(&launch_env, scenario) {
            Ok(()) => 0,
            Err(err) => {
                eprintln!("synthetic scenario {} failed: {err}", scenario.as_str());
                1
            }
        }
    })
}

#[no_mangle]
pub unsafe extern "C" fn mi_core_acquire_instance_lock(
    doc_dir_utf8: *const c_char,
) -> *mut DocumentInstanceLock {
    ffi_guard(ptr::null_mut(), "mi_core_acquire_instance_lock", || {
        let explicit_dir = match parse_optional_trimmed_path(doc_dir_utf8) {
            Ok(dir) => dir,
            Err(()) => {
                ffi_safe_log("invalid UTF-8 path in mi_core_acquire_instance_lock", "");
                return ptr::null_mut();
            }
        };
        let doc_dir = match explicit_dir {
            Some(dir) => dir,
            None => match QaLaunchEnv::from_env() {
                Ok(env) => env.document_dir,
                Err(_) => return ptr::null_mut(),
            },
        };

        match DocumentInstanceLock::acquire(&doc_dir) {
            Ok(Some(lock)) => Box::into_raw(Box::new(lock)),
            Ok(None) => {
                eprintln!("Micro Input already running for {}", doc_dir.display());
                ptr::null_mut()
            }
            Err(err) => {
                eprintln!("Micro Input instance lock error ({err})");
                ptr::null_mut()
            }
        }
    })
}

#[no_mangle]
pub unsafe extern "C" fn mi_core_release_instance_lock(ptr: *mut DocumentInstanceLock) {
    ffi_guard((), "mi_core_release_instance_lock", || {
        if !ptr.is_null() {
            drop(Box::from_raw(ptr));
        }
    });
}

#[no_mangle]
pub unsafe extern "C" fn mi_core_new(doc_dir_utf8: *const c_char) -> *mut WorkbenchStore {
    ffi_guard(ptr::null_mut(), "mi_core_new", || {
        let custom_dir = match parse_optional_trimmed_path(doc_dir_utf8) {
            Ok(dir) => dir,
            Err(()) => {
                ffi_safe_log("invalid UTF-8 path in mi_core_new", "");
                return ptr::null_mut();
            }
        };

        let (cfg, is_production) = match custom_dir {
            Some(dir) => (WorkbenchStoreConfig::isolated(&dir), false),
            None => (WorkbenchStoreConfig::production(), true),
        };

        match WorkbenchStore::open(cfg) {
            Ok(mut store) => {
                if is_production {
                    store.start_mac_usb_discovery_watcher();
                }
                Box::into_raw(Box::new(store))
            }
            Err(err) => {
                eprintln!("failed to open WorkbenchStore: {err}");
                ptr::null_mut()
            }
        }
    })
}

#[no_mangle]
pub unsafe extern "C" fn mi_core_free(ptr: *mut WorkbenchStore) {
    ffi_guard((), "mi_core_free", || {
        if !ptr.is_null() {
            drop(Box::from_raw(ptr));
        }
    });
}

#[no_mangle]
pub unsafe extern "C" fn mi_core_project_json(ptr: *mut WorkbenchStore) -> *mut c_char {
    ffi_guard(ptr::null_mut(), "mi_core_project_json", || {
        if ptr.is_null() {
            return ptr::null_mut();
        }
        let store = &*ptr;
        match serde_json::to_string(store.projection()) {
            Ok(json) => string_to_c_char(json),
            Err(err) => {
                eprintln!("failed to serialize projection: {err}");
                ptr::null_mut()
            }
        }
    })
}

#[no_mangle]
pub unsafe extern "C" fn mi_core_dispatch_json(
    ptr: *mut WorkbenchStore,
    cmd_utf8: *const c_char,
) -> *mut c_char {
    ffi_guard(ptr::null_mut(), "mi_core_dispatch_json", || {
        if ptr.is_null() || cmd_utf8.is_null() {
            return ptr::null_mut();
        }
        let store = &mut *ptr;
        let before_gen = store.projection_generation();
        let raw = match CStr::from_ptr(cmd_utf8).to_str() {
            Ok(s) => s,
            Err(_) => {
                ffi_safe_log("invalid UTF-8 command in mi_core_dispatch_json", "");
                return ptr::null_mut();
            }
        };
        match serde_json::from_str::<WorkbenchCommand>(raw) {
            Ok(cmd) => {
                if let Err(err) = store.dispatch_command(cmd) {
                    eprintln!("dispatch_command error: {err}");
                }
            }
            Err(err) => {
                eprintln!(
                    "invalid WorkbenchCommand JSON ({:?} at line {}, column {})",
                    err.classify(),
                    err.line(),
                    err.column()
                );
                return ptr::null_mut();
            }
        }
        if store.projection_generation() == before_gen {
            return ptr::null_mut();
        }
        match serde_json::to_string(store.projection()) {
            Ok(json) => string_to_c_char(json),
            Err(_) => ptr::null_mut(),
        }
    })
}

#[no_mangle]
pub unsafe extern "C" fn mi_core_poll_json(ptr: *mut WorkbenchStore) -> *mut c_char {
    ffi_guard(ptr::null_mut(), "mi_core_poll_json", || {
        if ptr.is_null() {
            return ptr::null_mut();
        }
        let store = &mut *ptr;
        if store.poll_hardware_events() {
            match serde_json::to_string(store.projection()) {
                Ok(json) => string_to_c_char(json),
                Err(_) => ptr::null_mut(),
            }
        } else {
            ptr::null_mut()
        }
    })
}

#[no_mangle]
pub unsafe extern "C" fn mi_core_string_free(s: *mut c_char) {
    ffi_guard((), "mi_core_string_free", || {
        if !s.is_null() {
            drop(CString::from_raw(s));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn ffi_roundtrip_projects_and_dispatches_commands() {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let temp_dir = std::env::temp_dir().join(format!("mi-ffi-test-{nanos}"));
        let dir_cstr = CString::new(temp_dir.to_string_lossy().to_string()).unwrap();

        unsafe {
            let lock = mi_core_acquire_instance_lock(dir_cstr.as_ptr());
            assert!(!lock.is_null());

            let store = mi_core_new(dir_cstr.as_ptr());
            assert!(!store.is_null());

            let proj_ptr = mi_core_project_json(store);
            assert!(!proj_ptr.is_null());
            let proj_str = CStr::from_ptr(proj_ptr).to_string_lossy().into_owned();
            mi_core_string_free(proj_ptr);
            assert!(proj_str.contains("\"header\""));
            assert!(proj_str.contains("\"canvas\""));

            let cmd =
                CString::new(r#"{"domain":"canvas","action":{"SelectSurface":"EncoderPress"}}"#)
                    .unwrap();
            let updated_ptr = mi_core_dispatch_json(store, cmd.as_ptr());
            assert!(!updated_ptr.is_null());
            let updated_str = CStr::from_ptr(updated_ptr).to_string_lossy().into_owned();
            mi_core_string_free(updated_ptr);
            assert!(updated_str.contains("\"EncoderPress\""));

            let noop_repeat = mi_core_dispatch_json(store, cmd.as_ptr());
            assert!(
                noop_repeat.is_null(),
                "repeating identical surface selection must short-circuit without JSON serialization"
            );

            let noop_fg =
                CString::new(r#"{"domain":"foreground_app_changed","action":{"bundle_id":null}}"#)
                    .unwrap();
            assert!(mi_core_dispatch_json(store, noop_fg.as_ptr()).is_null());

            let noop_win =
                CString::new(r#"{"domain":"main_window_active_changed","action":{"active":true}}"#)
                    .unwrap();
            assert!(mi_core_dispatch_json(store, noop_win.as_ptr()).is_null());

            let unchanged_ptr = mi_core_project_json(store);
            assert!(!unchanged_ptr.is_null());
            let unchanged = CStr::from_ptr(unchanged_ptr).to_string_lossy().into_owned();
            mi_core_string_free(unchanged_ptr);
            assert_eq!(
                unchanged, updated_str,
                "null dispatch must preserve the published JSON"
            );

            mi_core_free(store);
            mi_core_release_instance_lock(lock);
        }
        let _ = std::fs::remove_dir_all(&temp_dir);
    }
    #[test]
    fn ffi_board_intent_never_activates_a_host() {
        let temporary = tempfile::tempdir().unwrap();
        let directory = temporary.path();
        let mut store = WorkbenchStore::open_isolated(&directory).unwrap();
        store.config.isolated = false;
        store.document_store.set_host_active_profile(None).unwrap();
        let board = &store.projection().sidebar.board_layers;
        let codex = board
            .profile_choices
            .iter()
            .find(|row| row.display_name == "Codex")
            .unwrap()
            .profile_id
            .clone();
        let ag = board
            .profile_choices
            .iter()
            .find(|row| row.display_name == "Antigravity")
            .unwrap()
            .profile_id
            .clone();
        let commands = [
            (
                serde_json::json!({"PreviewBoardLayer":1}),
                Some(codex.clone()),
            ),
            (
                serde_json::json!({"SetBoardLayerProfile":{"layer":5,"profile_id":ag}}),
                Some(codex.clone()),
            ),
            (serde_json::json!({"PreviewBoardLayer":5}), Some(ag.clone())),
            (
                serde_json::json!({"SetBoardLayerProfile":{"layer":6,"profile_id":codex}}),
                Some(ag.clone()),
            ),
            (
                serde_json::json!({"PreviewBoardLayer":6}),
                Some(codex.clone()),
            ),
            (
                serde_json::json!({"SetBoardLayerProfile":{"layer":6,"profile_id":null}}),
                Some(codex.clone()),
            ),
            (serde_json::json!({"PreviewBoardLayer":6}), None),
            (serde_json::json!("ResetBoardLayerPlan"), None),
            (serde_json::json!("ProvisionBoardLayers"), None),
        ];
        for (action, editing_profile) in commands {
            let input =
                CString::new(serde_json::json!({"domain":"sidebar","action":action}).to_string())
                    .unwrap();
            unsafe {
                let output = mi_core_dispatch_json(&mut store, input.as_ptr());
                if !output.is_null() {
                    mi_core_string_free(output);
                }
            }
            assert!(store.active_host_profile_id().is_none());
            assert_eq!(store.projection().selected_profile_id, editing_profile);
            assert!(!store.ptt_controller.is_active);
            assert!(store.dispatched_receipts().is_empty());
        }
        store
            .document_store
            .set_host_active_profile(Some(codex))
            .unwrap();
        store.set_status_message("force normal runtime reconciliation");
        drop(store);
    }

    #[test]
    fn ffi_rejects_invalid_utf8_paths_and_commands_without_lossy_fallback() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut bad_path = temp_dir.path().to_string_lossy().into_owned().into_bytes();
        bad_path.extend_from_slice(b"/bad-\xff");
        let bad_cstr = CString::new(bad_path).unwrap();

        unsafe {
            let lock = mi_core_acquire_instance_lock(bad_cstr.as_ptr());
            assert!(
                lock.is_null(),
                "invalid UTF-8 doc dir must fail lock acquisition instead of lossy path creation"
            );

            let store = mi_core_new(bad_cstr.as_ptr());
            assert!(
                store.is_null(),
                "invalid UTF-8 doc dir must return null store instead of lossy fallback"
            );

            let valid_cstr = CString::new(temp_dir.path().to_string_lossy().into_owned()).unwrap();
            let valid_store = mi_core_new(valid_cstr.as_ptr());
            assert!(!valid_store.is_null());

            let bad_cmd = CString::new(b"{\"domain\":\"canvas\",\xff}".to_vec()).unwrap();
            let before = CStr::from_ptr(mi_core_project_json(valid_store))
                .to_string_lossy()
                .into_owned();
            assert!(mi_core_dispatch_json(valid_store, bad_cmd.as_ptr()).is_null());
            let after_ptr = mi_core_project_json(valid_store);
            let after = CStr::from_ptr(after_ptr).to_string_lossy().into_owned();
            mi_core_string_free(after_ptr);
            assert_eq!(before, after);

            mi_core_free(valid_store);
        }
    }

    #[test]
    fn ffi_dispatch_json_does_not_leak_raw_payload_to_stderr() {
        if std::env::var_os("MI_FFI_LEAK_PROBE").is_some() {
            let temp_dir = tempfile::tempdir().unwrap();
            let dir_cstr = CString::new(temp_dir.path().to_string_lossy().into_owned()).unwrap();
            let secret_cmd = CString::new("{\"secret\":\"SUPER_SECRET_FFI_PAYLOAD_987\",").unwrap();
            let secret_variant_cmd =
                CString::new("{\"domain\":\"canvas\",\"action\":\"SUPER_SECRET_VARIANT_987\"}")
                    .unwrap();
            unsafe {
                let store = mi_core_new(dir_cstr.as_ptr());
                assert!(!store.is_null());
                let res = mi_core_dispatch_json(store, secret_cmd.as_ptr());
                assert!(res.is_null());
                let res2 = mi_core_dispatch_json(store, secret_variant_cmd.as_ptr());
                assert!(res2.is_null());
                mi_core_free(store);
            }
            return;
        }
        let exe = std::env::current_exe().unwrap();
        let output = std::process::Command::new(exe)
            .args([
                "--exact",
                "tests::ffi_dispatch_json_does_not_leak_raw_payload_to_stderr",
                "--nocapture",
            ])
            .env("MI_FFI_LEAK_PROBE", "1")
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            output.status.success() && stdout.contains("1 passed"),
            "child leak probe failed to run cleanly (status={:?}, stdout={stdout})",
            output.status
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            !stderr.contains("SUPER_SECRET_FFI_PAYLOAD_987")
                && !stderr.contains("SUPER_SECRET_VARIANT_987"),
            "mi_core_dispatch_json leaked raw payload to stderr: {stderr}"
        );
    }

    #[test]
    fn ffi_guard_catches_panic_across_c_abi_boundary() {
        let ptr_res = ffi_guard(ptr::null_mut::<c_char>(), "test_ptr", || {
            panic!("simulated panic")
        });
        assert!(ptr_res.is_null());
        let code_res = ffi_guard(-1i32, "test_code", || panic!("simulated panic"));
        assert_eq!(code_res, -1);
    }
}
