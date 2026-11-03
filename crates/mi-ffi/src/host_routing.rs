use super::{failure, BridgeError, MicroInput};
#[uniffi::export(async_runtime = "tokio")]
impl MicroInput {
    pub fn host_routing_state(&self) -> String {
        self.draft.lock().unwrap().routing_state().to_string()
    }
    pub fn host_input_watermark(&self) -> String {
        match self.current() {
            Ok(s) => serde_json::json!({"session_id":s.id,"ingress_sequence":s.input_watermark()})
                .to_string(),
            Err(_) => serde_json::json!({"session_id":null,"ingress_sequence":0}).to_string(),
        }
    }
    pub async fn update_host_routing(&self, request_json: String) -> Result<String, BridgeError> {
        let request = serde_json::from_str(&request_json).map_err(failure)?;
        let mut store = self.draft.lock().unwrap();
        store.update_routing(request).map_err(failure)?;
        Ok(store.routing_state().to_string())
    }
    pub fn resolve_host_input(
        &self,
        binding_id: String,
        expected_document_id: String,
        expected_document_revision: u64,
        expected_host_state_revision: u64,
    ) -> Option<String> {
        if !self.operation_allows_binding(&binding_id) {
            return None;
        }
        let store = self.draft.lock().unwrap();
        let store = store.host_store();
        if !store.host_execution_enabled()
            || store.draft.document_id != expected_document_id
            || store.draft.revision != expected_document_revision
            || store.host_revision() != expected_host_state_revision
        {
            return None;
        }
        let profile = store.host_active_profile_id()?;
        let action = mi_model::host_router::resolve_host_action(
            &store.draft.document,
            Some(&profile),
            &binding_id,
            true,
        )?;
        Some(serde_json::json!({"profile_id":profile,"document_id":store.draft.document_id,"document_revision":store.draft.revision,"host_state_revision":store.host_revision(),"action":action}).to_string())
    }
}
