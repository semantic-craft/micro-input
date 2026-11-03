//! Cached, checksum-verified session snapshots only. These operations never request USB data.
use super::*;
use mi_model::comparison::Choice;
impl MicroInput {
    pub(super) fn comparison_snapshot(
        state: &ConnectionState,
    ) -> Result<(mi_model::DeviceConfig, serde_json::Value), BridgeError> {
        let (id, session) = state.session.as_ref().ok_or(BridgeError::Disconnected)?;
        let snapshot = session
            .snapshot()
            .filter(|s| !s.stale)
            .ok_or_else(|| failure("请先读取完整且未过期的设备快照"))?;
        let file = |f: &mi_session::VerifiedFile| serde_json::json!({"state":"present","name":f.meta.file.name(),"size":f.meta.size,"sha1":f.meta.checksum_hex(),"bytes":f.bytes.as_ref()});
        let baseline = serde_json::json!({"device_identity":{"interface":id,"session_id":session.id,"confidence":"observed-interface","backend":session.backend_name()},"firmware":snapshot.config.firmware,"read_at":snapshot.config.read_at,"keymap":file(&snapshot.files.keymap),"smart_actions":snapshot.files.smart_actions.as_ref().map(file).unwrap_or(serde_json::json!({"state":"missing"}))});
        let mut baseline = baseline;
        #[cfg(feature = "synthetic-backend")]
        if session.backend_name() == "synthetic/numbered-0x06" && state.synthetic_write_enabled {
            baseline["device_identity"] = serde_json::json!({"confidence":"verified-serial","serial":"synthetic","session_id":session.id});
        }
        baseline["active_indices"] = state.identity.as_ref().filter(|i| i.session_id == session.id).map(|i| serde_json::json!({"profile":i.profile_index,"layer":i.layer_index,"source_method":i.source_method,"observed_at":"identity-query; not active source mapping"})).unwrap_or(serde_json::Value::Null);
        Ok((snapshot.config, baseline))
    }
}
fn comparison_response(
    comparison: &mi_model::comparison::Comparison,
) -> Result<String, BridgeError> {
    let mut value = serde_json::to_value(comparison).map_err(failure)?;
    value["can_merge"] = serde_json::json!(comparison.can_merge());
    serde_json::to_string(&value).map_err(failure)
}
#[uniffi::export(async_runtime = "tokio")]
impl MicroInput {
    pub async fn compare_snapshot(&self) -> Result<String, BridgeError> {
        let state = self.state.lock().unwrap();
        let (config, baseline) = Self::comparison_snapshot(&state)?;
        let comparison = self
            .draft
            .lock()
            .unwrap()
            .compare(&config, baseline)
            .map_err(failure)?;
        comparison_response(&comparison)
    }
    pub async fn confirm_comparison_identity(&self, token: String) -> Result<String, BridgeError> {
        let state = self.state.lock().unwrap();
        let (_, baseline) = Self::comparison_snapshot(&state)?;
        let comparison = self
            .draft
            .lock()
            .unwrap()
            .confirm_comparison_identity(&token, &baseline)
            .map_err(failure)?;
        comparison_response(&comparison)
    }
    pub async fn resolve_comparison(
        &self,
        token: String,
        choice: String,
    ) -> Result<String, BridgeError> {
        let choice = match choice.as_str() {
            "merge" => Choice::Merge,
            "local" => Choice::KeepLocal,
            "device" => Choice::UseDevice,
            "cancel" => Choice::Cancel,
            _ => return Err(failure("未知比较选择")),
        };
        // Cancel requires no live session; it is always a no-op on the document.
        if choice == Choice::Cancel {
            let mut store = self.draft.lock().unwrap();
            store
                .resolve_comparison(&token, choice, &serde_json::Value::Null)
                .map_err(failure)?;
            return document_response(&store, None);
        }
        let state = self.state.lock().unwrap();
        let (_, baseline) = Self::comparison_snapshot(&state)?;
        let mut store = self.draft.lock().unwrap();
        store
            .resolve_comparison(&token, choice, &baseline)
            .map_err(failure)?;
        document_response(&store, None)
    }
}
