//! Preparation uses only the current completed session cache. No USB request can be sent here.
use super::*;
#[uniffi::export(async_runtime = "tokio")]
impl MicroInput {
    pub async fn preview_apply(&self) -> Result<String, BridgeError> {
        let state = self.state.lock().unwrap();
        let (_, baseline) = Self::comparison_snapshot(&state)?;
        let result = self
            .draft
            .lock()
            .unwrap()
            .preview_apply(&baseline)
            .map_err(failure)?;
        serde_json::to_string(&result).map_err(failure)
    }
    pub async fn cancel_apply_preview(&self) {
        self.draft.lock().unwrap().cancel_apply_preview();
    }
    pub async fn prepare_apply_backup(&self, token: String) -> Result<String, BridgeError> {
        let state = self.state.lock().unwrap();
        let (_, baseline) = Self::comparison_snapshot(&state)?;
        let result = self
            .draft
            .lock()
            .unwrap()
            .prepare_apply_backup(&token, &baseline)
            .map_err(failure)?;
        serde_json::to_string(&result).map_err(failure)
    }
}
