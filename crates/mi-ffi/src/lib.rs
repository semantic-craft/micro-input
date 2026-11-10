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

fn string_to_c_char(s: String) -> *mut c_char {
    match CString::new(s) {
        Ok(cstr) => cstr.into_raw(),
        Err(_) => ptr::null_mut(),
    }
}

#[no_mangle]
pub extern "C" fn mi_core_run_qa_from_env() -> i32 {
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
}

#[no_mangle]
pub unsafe extern "C" fn mi_core_acquire_instance_lock(
    doc_dir_utf8: *const c_char,
) -> *mut DocumentInstanceLock {
    let explicit_dir = (!doc_dir_utf8.is_null())
        .then(|| {
            let raw = CStr::from_ptr(doc_dir_utf8).to_string_lossy();
            let trimmed = raw.trim();
            (!trimmed.is_empty()).then(|| PathBuf::from(trimmed))
        })
        .flatten();
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
}

#[no_mangle]
pub unsafe extern "C" fn mi_core_release_instance_lock(ptr: *mut DocumentInstanceLock) {
    if !ptr.is_null() {
        drop(Box::from_raw(ptr));
    }
}

#[no_mangle]
pub unsafe extern "C" fn mi_core_new(doc_dir_utf8: *const c_char) -> *mut WorkbenchStore {
    let custom_dir = if !doc_dir_utf8.is_null() {
        let raw = CStr::from_ptr(doc_dir_utf8).to_string_lossy();
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            Some(PathBuf::from(trimmed))
        } else {
            None
        }
    } else {
        None
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
}

#[no_mangle]
pub unsafe extern "C" fn mi_core_free(ptr: *mut WorkbenchStore) {
    if !ptr.is_null() {
        drop(Box::from_raw(ptr));
    }
}

#[no_mangle]
pub unsafe extern "C" fn mi_core_project_json(ptr: *mut WorkbenchStore) -> *mut c_char {
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
}

#[no_mangle]
pub unsafe extern "C" fn mi_core_dispatch_json(
    ptr: *mut WorkbenchStore,
    cmd_utf8: *const c_char,
) -> *mut c_char {
    if ptr.is_null() || cmd_utf8.is_null() {
        return ptr::null_mut();
    }
    let store = &mut *ptr;
    let before_gen = store.projection_generation();
    let raw = CStr::from_ptr(cmd_utf8).to_string_lossy();
    match serde_json::from_str::<WorkbenchCommand>(&raw) {
        Ok(cmd) => {
            if let Err(err) = store.dispatch_command(cmd) {
                eprintln!("dispatch_command error: {err}");
            }
        }
        Err(err) => {
            eprintln!("invalid WorkbenchCommand JSON ({raw}): {err}");
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
}

#[no_mangle]
pub unsafe extern "C" fn mi_core_poll_json(ptr: *mut WorkbenchStore) -> *mut c_char {
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
}

#[no_mangle]
pub unsafe extern "C" fn mi_core_string_free(s: *mut c_char) {
    if !s.is_null() {
        drop(CString::from_raw(s));
    }
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

            mi_core_free(store);
            mi_core_release_instance_lock(lock);
        }
        let _ = std::fs::remove_dir_all(&temp_dir);
    }
    #[test]
    fn ffi_board_intent_never_refreshes_ownership_or_activates_a_host() {
        use mi_bridge::{NativeOwnershipController, UnixNativeOwnershipTransport};
        use std::os::unix::{fs::PermissionsExt, net::UnixListener};
        let temporary = tempfile::tempdir().unwrap();
        let directory = temporary.path();
        let socket = directory.join("ownership.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let mut store = WorkbenchStore::open_isolated(&directory).unwrap();
        store.config.isolated = false;
        store.native_ownership =
            NativeOwnershipController::new(Some(UnixNativeOwnershipTransport::new(&socket)), false);
        store.native_ownership.set_desired_owner("native");
        store.document_store.set_host_active_profile(None).unwrap();
        let generation = store.native_ownership.generation();
        let selected = store.projection().selected_profile_id.clone();
        let board = &store.projection().sidebar.board_layers;
        let codex = board
            .profile_choices
            .iter()
            .find(|row| row.display_name == "Codex")
            .unwrap()
            .profile_id
            .clone();
        let cindy = board
            .profile_choices
            .iter()
            .find(|row| row.display_name == "Cindy")
            .unwrap()
            .profile_id
            .clone();
        let commands = [
            serde_json::json!({"PreviewBoardLayer":1}),
            serde_json::json!({"SetBoardLayerProfile":{"layer":5,"profile_id":cindy}}),
            serde_json::json!({"PreviewBoardLayer":5}),
            serde_json::json!({"SetBoardLayerProfile":{"layer":6,"profile_id":codex}}),
            serde_json::json!({"PreviewBoardLayer":6}),
            serde_json::json!({"SetBoardLayerProfile":{"layer":6,"profile_id":null}}),
            serde_json::json!("ResetBoardLayerPlan"),
            serde_json::json!("ProvisionBoardLayers"),
        ];
        for action in commands {
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
            assert_eq!(store.projection().selected_profile_id, selected);
            assert_eq!(store.native_ownership.generation(), generation);
            assert!(!store.native_ownership.has_pending_request());
            assert!(!store.ptt_controller.is_active);
            assert!(store.dispatched_receipts().is_empty());
            assert_eq!(
                listener.accept().unwrap_err().kind(),
                std::io::ErrorKind::WouldBlock
            );
        }
        // The listener is a live positive control for the path the commands must avoid.
        store.set_status_message("force normal runtime reconciliation");
        assert!(listener.accept().is_ok());
        drop(store);
        drop(listener);
    }
}
