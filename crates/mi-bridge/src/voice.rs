#[derive(Debug, Clone)]
pub struct PushToTalkController {
    pub is_active: bool,
    pub cleanup_confirmed: bool,
    pub simulated: bool,
    pub active_binding: Option<String>,
    pub active_adapter: Option<String>,
    pub active_profile: Option<String>,
    next_start_outcome: (bool, String),
    next_verify_stopped: bool,
}

impl Default for PushToTalkController {
    fn default() -> Self {
        Self {
            is_active: false,
            cleanup_confirmed: true,
            simulated: true,
            active_binding: None,
            active_adapter: None,
            active_profile: None,
            next_start_outcome: (true, "按住听写已启动".into()),
            next_verify_stopped: true,
        }
    }
}

impl PushToTalkController {
    pub fn new_simulated() -> Self {
        Self::default()
    }

    pub fn set_next_start_outcome(&mut self, confirmed: bool, detail: impl Into<String>) {
        self.next_start_outcome = (confirmed, detail.into());
    }

    pub fn set_next_verify_stopped(&mut self, stopped: bool) {
        self.next_verify_stopped = stopped;
    }

    pub fn start_held_dictation(&mut self, adapter: &str, binding: &str, profile: &str) {
        self.active_adapter = Some(adapter.to_owned());
        self.active_binding = Some(binding.to_owned());
        self.active_profile = Some(profile.to_owned());
        self.is_active = self.next_start_outcome.0;
    }

    pub async fn wait_for_start_outcome(&mut self) -> (bool, String) {
        let outcome = self.next_start_outcome.clone();
        if !outcome.0 {
            self.is_active = false;
            self.active_adapter = None;
            self.active_binding = None;
            self.active_profile = None;
        }
        outcome
    }

    pub fn release(&mut self, binding: &str) {
        if self.active_binding.as_deref() == Some(binding) || self.active_binding.is_none() {
            self.is_active = false;
            self.active_binding = None;
            self.active_adapter = None;
            self.active_profile = None;
        }
    }

    pub fn cancel(&mut self) {
        self.is_active = false;
        self.active_binding = None;
        self.active_adapter = None;
        self.active_profile = None;
    }

    pub async fn wait_until_stopped(&mut self) {
        self.is_active = false;
    }

    pub async fn verify_stopped(&mut self) -> bool {
        self.cleanup_confirmed = self.next_verify_stopped;
        self.next_verify_stopped
    }
}
