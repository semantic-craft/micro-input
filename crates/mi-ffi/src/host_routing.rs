use super::{failure, BridgeError, MicroInput};
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum RadialInputReadiness {
    Ready,
    Disconnected,
    Stale,
    UnverifiedMapping,
    Paused,
}

#[uniffi::export(async_runtime = "tokio")]
impl MicroInput {
    pub fn radial_menu_input_readiness(&self, session_id: u64) -> RadialInputReadiness {
        let bindings = ["AG15", "AG16", "AG17", "AG18", "ENC_CLK"];
        if !bindings
            .iter()
            .all(|binding| self.operation_allows_binding(binding))
        {
            return RadialInputReadiness::Paused;
        }
        let state = self.state.lock().unwrap();
        let Some((_, session)) = &state.session else {
            return RadialInputReadiness::Disconnected;
        };
        if session.id != session_id {
            return RadialInputReadiness::Disconnected;
        }
        let Some(snapshot) = session.snapshot().filter(|s| !s.stale) else {
            return RadialInputReadiness::Stale;
        };
        let Some(identity) = state
            .identity
            .as_ref()
            .filter(|i| i.session_id == session_id)
        else {
            return RadialInputReadiness::Stale;
        };
        let identity = mi_model::host_router::HostIdentityStatus {
            version: identity.version.clone(),
            profile_index: identity.profile_index,
            layer_index: identity.layer_index,
        };
        let available = mi_model::host_router::verified_bindings(&snapshot.config, &identity);
        if bindings.iter().all(|binding| available.contains(*binding)) {
            RadialInputReadiness::Ready
        } else {
            RadialInputReadiness::UnverifiedMapping
        }
    }
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
        let action = mi_model::host_router::resolve_host_action_with_command_consent(
            &store.draft.document,
            Some(&profile),
            &binding_id,
            true,
            store.command_execution_enabled(),
        )?;
        Some(serde_json::json!({"profile_id":profile,"document_id":store.draft.document_id,"document_revision":store.draft.revision,"host_state_revision":store.host_revision(),"action":action}).to_string())
    }
}

#[cfg(all(test, feature = "synthetic-backend"))]
mod tests {
    use super::*;
    use mi_transport::HidTransport;
    use std::sync::Arc;

    #[tokio::test]
    async fn radial_readiness_follows_fresh_mapping_changes_and_restore() {
        let transport = Arc::new(mi_transport::SyntheticDiscovery::default());
        transport.set_synthetic_identity(3).unwrap();
        let configure = |encoder: bool, radial: bool| {
            let mut raw: serde_json::Value = serde_json::from_slice(include_bytes!(
                "../../mi-model/fixtures/device/synthetic-keymap-lighting.json"
            ))
            .unwrap();
            raw["profiles"][0]["layers"][0]["layout"]["encoders"] = serde_json::json!([
                "KV_OAI_ENC_CC",
                "KV_OAI_ENC_CW",
                if encoder { "KV_OAI_ENC_CLK" } else { "KC_A" }
            ]);
            raw["profiles"][0]["layers"][0]["layout"]["joystick"] =
                serde_json::json!(if radial { "VENDOR" } else { "UNKNOWN" });
            transport.set_files(mi_transport::SyntheticFiles::new(
                serde_json::to_vec(&raw).unwrap(),
                None,
            ));
        };
        configure(true, true);
        let mut core = MicroInput::synthetic_discovery();
        Arc::get_mut(&mut core).unwrap().transport = transport.clone();
        core.set_synthetic_discovery(crate::SyntheticDiscoveryState::ReconnectedDevice)
            .await
            .unwrap();
        core.list_devices().await.unwrap();
        core.connect("synthetic-reconnected-interface-0".into())
            .await
            .unwrap();
        let session = core.current().unwrap();
        assert_eq!(
            core.radial_menu_input_readiness(session.id),
            RadialInputReadiness::Stale
        );
        core.read_identity().await.unwrap();
        core.read_config().await.unwrap();
        assert_eq!(
            core.radial_menu_input_readiness(session.id),
            RadialInputReadiness::Ready
        );
        for (encoder, radial) in [(false, true), (true, false)] {
            configure(encoder, radial);
            core.read_config().await.unwrap();
            assert_eq!(
                core.radial_menu_input_readiness(session.id),
                RadialInputReadiness::UnverifiedMapping
            );
            configure(true, true);
            core.read_config().await.unwrap();
            assert_eq!(
                core.radial_menu_input_readiness(session.id),
                RadialInputReadiness::Ready
            );
        }
        session.disconnect();
        assert_eq!(
            core.radial_menu_input_readiness(session.id),
            RadialInputReadiness::Stale
        );
        core.shutdown().await;
    }
}
