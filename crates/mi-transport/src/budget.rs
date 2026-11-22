//! Per-device rolling-window write budget and connection request pacer.

use std::{
    collections::{BTreeMap, VecDeque},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use crate::TransportHint;

pub const USB_WRITE_WINDOW: Duration = Duration::from_secs(10);
pub const USB_MAX_WRITES_PER_WINDOW: usize = 60;
pub const BLE_WRITE_WINDOW: Duration = Duration::from_secs(10);
pub const BLE_MAX_WRITES_PER_WINDOW: usize = 40;
pub const BLE_MIN_WRITE_SPACING: Duration = Duration::from_millis(250);
pub const INPUT_KIT_CONNECTION_WINDOW: Duration = Duration::from_secs(10);
pub const INPUT_KIT_CONNECTION_MAX_REQUESTS: usize = 65;
pub const INPUT_KIT_CONNECTION_MIN_SPACING: Duration = Duration::from_millis(160);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DeviceBudgetKey {
    Serial(String),
    UsbLocation(u32),
    VendorPath(String),
    Synthetic(u64),
}

impl DeviceBudgetKey {
    pub fn from_identity(
        serial: Option<&str>,
        location_id: Option<u32>,
        vendor_path: &str,
    ) -> Self {
        if let Some(trimmed) = serial.map(str::trim).filter(|s| !s.is_empty()) {
            return Self::Serial(trimmed.to_owned());
        }
        if let Some(location) = location_id.filter(|&id| id != 0) {
            return Self::UsbLocation(location);
        }
        let path = vendor_path.trim();
        if !path.is_empty() {
            return Self::VendorPath(path.to_owned());
        }
        if let Some(location) = location_id {
            return Self::UsbLocation(location);
        }
        Self::VendorPath(String::new())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WriteBudgetPolicy {
    pub window: Duration,
    pub max_writes: usize,
    pub min_spacing: Duration,
    pub enabled: bool,
}

impl WriteBudgetPolicy {
    pub const fn usb() -> Self {
        Self {
            window: USB_WRITE_WINDOW,
            max_writes: USB_MAX_WRITES_PER_WINDOW,
            min_spacing: Duration::ZERO,
            enabled: true,
        }
    }

    pub const fn ble() -> Self {
        Self {
            window: BLE_WRITE_WINDOW,
            max_writes: BLE_MAX_WRITES_PER_WINDOW,
            min_spacing: BLE_MIN_WRITE_SPACING,
            enabled: true,
        }
    }

    pub const fn unbounded() -> Self {
        Self {
            window: Duration::ZERO,
            max_writes: usize::MAX,
            min_spacing: Duration::ZERO,
            enabled: false,
        }
    }

    pub const fn for_transport(hint: TransportHint) -> Self {
        match hint {
            TransportHint::Usb => Self::usb(),
            TransportHint::Bluetooth | TransportHint::Unknown => Self::ble(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum WritePriority {
    BackgroundSync,
    #[default]
    Normal,
    UrgentClear,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetAcquireError {
    SpacingWait { ready_at: Instant },
    WindowExhausted { retry_at: Instant },
}

impl BudgetAcquireError {
    pub fn retry_at(self) -> Instant {
        match self {
            Self::SpacingWait { ready_at } => ready_at,
            Self::WindowExhausted { retry_at } => retry_at,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WritePermit {
    pub issued_at: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WriteBudgetSnapshot {
    pub policy: WriteBudgetPolicy,
    pub used_in_window: usize,
    pub remaining_in_window: usize,
    pub next_ready_at: Option<Instant>,
}

#[derive(Debug)]
struct RollingWindow {
    policy: WriteBudgetPolicy,
    timestamps: VecDeque<Instant>,
    last_write_at: Option<Instant>,
}

impl RollingWindow {
    fn new(policy: WriteBudgetPolicy) -> Self {
        Self {
            policy,
            timestamps: VecDeque::new(),
            last_write_at: None,
        }
    }

    fn prune(&mut self, now: Instant) {
        let window = self.policy.window;
        while self
            .timestamps
            .front()
            .is_some_and(|&ts| now.saturating_duration_since(ts) >= window)
        {
            self.timestamps.pop_front();
        }
    }

    fn spacing_ready_at(&self, now: Instant, priority: WritePriority) -> Option<Instant> {
        if priority == WritePriority::UrgentClear || self.policy.min_spacing.is_zero() {
            return None;
        }
        let last = self.last_write_at?;
        let ready = last + self.policy.min_spacing;
        (now < ready).then_some(ready)
    }

    fn window_retry_at_for_batch(&self, now: Instant, count: usize) -> Option<Instant> {
        if count == 0 {
            return None;
        }
        if self.policy.max_writes == 0 || count > self.policy.max_writes {
            let fallback = self
                .timestamps
                .back()
                .map_or(now + self.policy.window, |&ts| ts + self.policy.window);
            return Some(fallback);
        }
        let total = self.timestamps.len().saturating_add(count);
        if total <= self.policy.max_writes {
            return None;
        }
        let overflow = total - self.policy.max_writes;
        let idx = overflow - 1;
        Some(self.timestamps[idx] + self.policy.window)
    }

    fn next_ready_at(&mut self, now: Instant, priority: WritePriority) -> Option<Instant> {
        if !self.policy.enabled {
            return None;
        }
        self.prune(now);
        let window_wait = self.window_retry_at_for_batch(now, 1);
        let spacing_wait = self.spacing_ready_at(now, priority);
        match (window_wait, spacing_wait) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        }
    }

    fn check_batch_capacity(
        &mut self,
        now: Instant,
        count: usize,
        priority: WritePriority,
    ) -> Result<(), BudgetAcquireError> {
        if !self.policy.enabled || count == 0 {
            return Ok(());
        }
        self.prune(now);
        if let Some(retry_at) = self.window_retry_at_for_batch(now, count) {
            return Err(BudgetAcquireError::WindowExhausted { retry_at });
        }
        if let Some(ready_at) = self.spacing_ready_at(now, priority) {
            return Err(BudgetAcquireError::SpacingWait { ready_at });
        }
        Ok(())
    }

    fn try_acquire(
        &mut self,
        now: Instant,
        priority: WritePriority,
    ) -> Result<WritePermit, BudgetAcquireError> {
        if !self.policy.enabled {
            return Ok(WritePermit { issued_at: now });
        }
        self.prune(now);
        if let Some(retry_at) = self.window_retry_at_for_batch(now, 1) {
            return Err(BudgetAcquireError::WindowExhausted { retry_at });
        }
        if let Some(ready_at) = self.spacing_ready_at(now, priority) {
            return Err(BudgetAcquireError::SpacingWait { ready_at });
        }
        self.timestamps.push_back(now);
        self.last_write_at = Some(now);
        Ok(WritePermit { issued_at: now })
    }

    fn snapshot(&mut self, now: Instant) -> WriteBudgetSnapshot {
        if !self.policy.enabled {
            return WriteBudgetSnapshot {
                policy: self.policy,
                used_in_window: 0,
                remaining_in_window: usize::MAX,
                next_ready_at: None,
            };
        }
        self.prune(now);
        let used = self.timestamps.len();
        let remaining = self.policy.max_writes.saturating_sub(used);
        let next_ready_at = self.next_ready_at(now, WritePriority::Normal);
        WriteBudgetSnapshot {
            policy: self.policy,
            used_in_window: used,
            remaining_in_window: remaining,
            next_ready_at,
        }
    }
}

#[derive(Debug)]
pub struct DeviceWriteBudget {
    inner: Mutex<RollingWindow>,
}

impl DeviceWriteBudget {
    pub fn new(policy: WriteBudgetPolicy) -> Self {
        Self {
            inner: Mutex::new(RollingWindow::new(policy)),
        }
    }

    pub fn policy(&self) -> WriteBudgetPolicy {
        self.inner.lock().unwrap().policy
    }

    pub fn update_policy(&self, policy: WriteBudgetPolicy) {
        self.inner.lock().unwrap().policy = policy;
    }

    pub fn next_ready_at(&self, now: Instant, priority: WritePriority) -> Option<Instant> {
        self.inner.lock().unwrap().next_ready_at(now, priority)
    }

    pub fn check_batch_capacity(
        &self,
        now: Instant,
        count: usize,
        priority: WritePriority,
    ) -> Result<(), BudgetAcquireError> {
        self.inner
            .lock()
            .unwrap()
            .check_batch_capacity(now, count, priority)
    }

    pub fn try_acquire(
        &self,
        now: Instant,
        priority: WritePriority,
    ) -> Result<WritePermit, BudgetAcquireError> {
        self.inner.lock().unwrap().try_acquire(now, priority)
    }

    pub fn snapshot(&self, now: Instant) -> WriteBudgetSnapshot {
        self.inner.lock().unwrap().snapshot(now)
    }
}

#[derive(Debug, Default)]
pub struct DeviceBudgetRegistry {
    budgets: Mutex<BTreeMap<DeviceBudgetKey, Arc<DeviceWriteBudget>>>,
}

impl DeviceBudgetRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn budget_for(
        &self,
        key: DeviceBudgetKey,
        policy: WriteBudgetPolicy,
    ) -> Arc<DeviceWriteBudget> {
        let mut guard = self.budgets.lock().unwrap();
        if let Some(existing) = guard.get(&key) {
            existing.update_policy(policy);
            return Arc::clone(existing);
        }
        let budget = Arc::new(DeviceWriteBudget::new(policy));
        guard.insert(key, Arc::clone(&budget));
        budget
    }

    pub fn get(&self, key: &DeviceBudgetKey) -> Option<Arc<DeviceWriteBudget>> {
        self.budgets.lock().unwrap().get(key).cloned()
    }
}

#[derive(Debug)]
pub struct ConnectionRequestBudget {
    inner: DeviceWriteBudget,
}

impl ConnectionRequestBudget {
    pub fn new(policy: WriteBudgetPolicy) -> Self {
        Self {
            inner: DeviceWriteBudget::new(policy),
        }
    }

    pub fn for_input_kit_connection() -> Self {
        Self::new(WriteBudgetPolicy {
            window: INPUT_KIT_CONNECTION_WINDOW,
            max_writes: INPUT_KIT_CONNECTION_MAX_REQUESTS,
            min_spacing: INPUT_KIT_CONNECTION_MIN_SPACING,
            enabled: true,
        })
    }

    pub fn policy(&self) -> WriteBudgetPolicy {
        self.inner.policy()
    }

    pub fn next_ready_at(&self, now: Instant) -> Option<Instant> {
        self.inner.next_ready_at(now, WritePriority::Normal)
    }

    pub fn try_acquire(&self, now: Instant) -> Result<WritePermit, BudgetAcquireError> {
        self.inner.try_acquire(now, WritePriority::Normal)
    }

    pub fn snapshot(&self, now: Instant) -> WriteBudgetSnapshot {
        self.inner.snapshot(now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_budget_key_prefers_serial_then_usb_location_then_vendor_path() {
        assert_eq!(
            DeviceBudgetKey::from_identity(Some("  SN-001  "), Some(0x1420), "DevSrvsID:7"),
            DeviceBudgetKey::Serial("SN-001".into())
        );
        assert_eq!(
            DeviceBudgetKey::from_identity(Some("   "), Some(0x1420), "DevSrvsID:7"),
            DeviceBudgetKey::UsbLocation(0x1420)
        );
        assert_eq!(
            DeviceBudgetKey::from_identity(None, Some(0), "DevSrvsID:7"),
            DeviceBudgetKey::VendorPath("DevSrvsID:7".into())
        );
        assert_eq!(
            DeviceBudgetKey::from_identity(None, None, "  DevSrvsID:9  "),
            DeviceBudgetKey::VendorPath("DevSrvsID:9".into())
        );
    }

    #[test]
    fn transport_hint_maps_to_expected_budget_policies() {
        let usb = WriteBudgetPolicy::for_transport(TransportHint::Usb);
        assert_eq!(usb, WriteBudgetPolicy::usb());
        assert_eq!(usb.window, Duration::from_secs(10));
        assert_eq!(usb.max_writes, 60);
        assert_eq!(usb.min_spacing, Duration::ZERO);
        assert!(usb.enabled);

        let ble = WriteBudgetPolicy::for_transport(TransportHint::Bluetooth);
        assert_eq!(ble, WriteBudgetPolicy::ble());
        assert_eq!(ble.window, Duration::from_secs(10));
        assert_eq!(ble.max_writes, 40);
        assert_eq!(ble.min_spacing, Duration::from_millis(250));
        assert!(ble.enabled);

        assert_eq!(
            WriteBudgetPolicy::for_transport(TransportHint::Unknown),
            WriteBudgetPolicy::ble()
        );
        assert!(!WriteBudgetPolicy::unbounded().enabled);
    }

    #[test]
    fn usb_rolling_window_enforces_60_writes_per_10s_and_recovers_as_entries_expire() {
        let budget = DeviceWriteBudget::new(WriteBudgetPolicy::usb());
        let t0 = Instant::now();

        for i in 0..60 {
            let ts = t0 + Duration::from_millis(i * 10);
            let permit = budget
                .try_acquire(ts, WritePriority::Normal)
                .expect("write within 60-write USB rolling window must succeed");
            assert_eq!(permit.issued_at, ts);
        }

        let t_after_burst = t0 + Duration::from_millis(600);
        let err = budget
            .try_acquire(t_after_burst, WritePriority::Normal)
            .expect_err("61st write within 10s USB window must be rejected");
        assert_eq!(
            err,
            BudgetAcquireError::WindowExhausted {
                retry_at: t0 + Duration::from_secs(10)
            }
        );
        assert_eq!(
            budget.next_ready_at(t_after_burst, WritePriority::Normal),
            Some(t0 + Duration::from_secs(10))
        );

        let snap = budget.snapshot(t_after_burst);
        assert_eq!(snap.used_in_window, 60);
        assert_eq!(snap.remaining_in_window, 0);
        assert_eq!(snap.next_ready_at, Some(t0 + Duration::from_secs(10)));

        let t_one_expired = t0 + Duration::from_secs(10);
        assert_eq!(
            budget.next_ready_at(t_one_expired, WritePriority::Normal),
            None
        );
        budget
            .try_acquire(t_one_expired, WritePriority::Normal)
            .expect("slot must reopen once oldest timestamp leaves 10s window");

        let err2 = budget
            .try_acquire(t_one_expired, WritePriority::Normal)
            .expect_err("window must be full again until second oldest timestamp expires");
        assert_eq!(
            err2,
            BudgetAcquireError::WindowExhausted {
                retry_at: t0 + Duration::from_millis(10) + Duration::from_secs(10)
            }
        );
    }

    #[test]
    fn ble_enforces_250ms_spacing_and_40_writes_per_10s_while_urgent_clear_bypasses_soft_spacing() {
        let budget = DeviceWriteBudget::new(WriteBudgetPolicy::ble());
        let t0 = Instant::now();

        budget
            .try_acquire(t0, WritePriority::Normal)
            .expect("first BLE write must succeed");

        let t_too_soon = t0 + Duration::from_millis(100);
        assert_eq!(
            budget.try_acquire(t_too_soon, WritePriority::Normal),
            Err(BudgetAcquireError::SpacingWait {
                ready_at: t0 + Duration::from_millis(250)
            })
        );
        assert_eq!(
            budget.next_ready_at(t_too_soon, WritePriority::Normal),
            Some(t0 + Duration::from_millis(250))
        );
        assert_eq!(
            budget.next_ready_at(t_too_soon, WritePriority::UrgentClear),
            None
        );

        budget
            .try_acquire(t_too_soon, WritePriority::UrgentClear)
            .expect("UrgentClear must bypass soft 250ms BLE spacing");

        for i in 2..40 {
            let ts = t0 + Duration::from_millis(i as u64 * 100);
            budget
                .try_acquire(ts, WritePriority::UrgentClear)
                .expect("UrgentClear within 40-write BLE window must succeed");
        }

        let t_full = t0 + Duration::from_millis(4_500);
        assert_eq!(
            budget.try_acquire(t_full, WritePriority::UrgentClear),
            Err(BudgetAcquireError::WindowExhausted {
                retry_at: t0 + Duration::from_secs(10)
            }),
            "UrgentClear must still obey the 40-write BLE rolling hard cap"
        );
        assert_eq!(
            budget.next_ready_at(t_full, WritePriority::UrgentClear),
            Some(t0 + Duration::from_secs(10))
        );
    }

    #[test]
    fn check_batch_capacity_validates_multi_write_headroom_and_spacing() {
        let budget = DeviceWriteBudget::new(WriteBudgetPolicy::usb());
        let t0 = Instant::now();
        for i in 0..57 {
            budget
                .try_acquire(t0 + Duration::from_millis(i * 5), WritePriority::Normal)
                .unwrap();
        }
        let check_at = t0 + Duration::from_secs(1);
        assert!(budget
            .check_batch_capacity(check_at, 3, WritePriority::Normal)
            .is_ok());
        assert_eq!(
            budget.check_batch_capacity(check_at, 4, WritePriority::Normal),
            Err(BudgetAcquireError::WindowExhausted {
                retry_at: t0 + Duration::from_secs(10)
            })
        );
        assert_eq!(
            budget.check_batch_capacity(check_at, 5, WritePriority::Normal),
            Err(BudgetAcquireError::WindowExhausted {
                retry_at: t0 + Duration::from_millis(5) + Duration::from_secs(10)
            })
        );
        assert!(budget
            .check_batch_capacity(check_at, 61, WritePriority::Normal)
            .is_err());
        assert_eq!(budget.snapshot(check_at).used_in_window, 57);

        let ble = DeviceWriteBudget::new(WriteBudgetPolicy::ble());
        ble.try_acquire(t0, WritePriority::Normal).unwrap();
        assert_eq!(
            ble.check_batch_capacity(t0 + Duration::from_millis(50), 2, WritePriority::Normal),
            Err(BudgetAcquireError::SpacingWait {
                ready_at: t0 + Duration::from_millis(250)
            })
        );
        assert!(ble
            .check_batch_capacity(
                t0 + Duration::from_millis(50),
                2,
                WritePriority::UrgentClear
            )
            .is_ok());
    }

    #[test]
    fn registry_reuses_same_budget_arc_and_preserves_window_across_policy_transitions() {
        let registry = DeviceBudgetRegistry::new();
        let key = DeviceBudgetKey::from_identity(Some("MICRO-42"), Some(9), "DevSrvsID:9");
        let t0 = Instant::now();

        let usb_budget = registry.budget_for(key.clone(), WriteBudgetPolicy::usb());
        for i in 0..45 {
            usb_budget
                .try_acquire(t0 + Duration::from_millis(i * 10), WritePriority::Normal)
                .unwrap();
        }

        let ble_budget = registry.budget_for(key.clone(), WriteBudgetPolicy::ble());
        assert!(
            Arc::ptr_eq(&usb_budget, &ble_budget),
            "registry must return the exact same Arc for the same physical device"
        );
        assert_eq!(ble_budget.policy(), WriteBudgetPolicy::ble());
        assert_eq!(
            ble_budget
                .snapshot(t0 + Duration::from_secs(1))
                .used_in_window,
            45
        );

        let err = ble_budget
            .try_acquire(t0 + Duration::from_secs(1), WritePriority::UrgentClear)
            .expect_err("45 existing writes must remain above BLE 40-write cap");
        assert_eq!(
            err,
            BudgetAcquireError::WindowExhausted {
                retry_at: t0 + Duration::from_millis(50) + Duration::from_secs(10)
            }
        );

        let back_to_usb = registry.budget_for(key, WriteBudgetPolicy::usb());
        assert!(Arc::ptr_eq(&usb_budget, &back_to_usb));
        assert!(back_to_usb
            .try_acquire(t0 + Duration::from_secs(1), WritePriority::Normal)
            .is_ok());
        assert_eq!(
            back_to_usb
                .snapshot(t0 + Duration::from_secs(1))
                .used_in_window,
            46
        );
    }

    #[test]
    fn connection_request_budget_enforces_65_requests_per_10s_and_160ms_spacing() {
        let budget = ConnectionRequestBudget::for_input_kit_connection();
        assert_eq!(budget.policy().window, Duration::from_secs(10));
        assert_eq!(budget.policy().max_writes, 65);
        assert_eq!(budget.policy().min_spacing, Duration::from_millis(160));

        let t0 = Instant::now();
        budget.try_acquire(t0).unwrap();
        assert_eq!(
            budget.try_acquire(t0 + Duration::from_millis(100)),
            Err(BudgetAcquireError::SpacingWait {
                ready_at: t0 + Duration::from_millis(160)
            })
        );

        for i in 1..62 {
            let ts = t0 + Duration::from_millis(i * 160);
            budget
                .try_acquire(ts)
                .expect("spaced requests within 65-request window must succeed");
        }
        assert_eq!(
            budget
                .snapshot(t0 + Duration::from_millis(61 * 160))
                .used_in_window,
            62
        );

        let burst_budget = ConnectionRequestBudget::new(WriteBudgetPolicy {
            window: INPUT_KIT_CONNECTION_WINDOW,
            max_writes: INPUT_KIT_CONNECTION_MAX_REQUESTS,
            min_spacing: Duration::ZERO,
            enabled: true,
        });
        for i in 0..65 {
            burst_budget
                .try_acquire(t0 + Duration::from_millis(i))
                .unwrap();
        }
        assert_eq!(
            burst_budget.try_acquire(t0 + Duration::from_millis(70)),
            Err(BudgetAcquireError::WindowExhausted {
                retry_at: t0 + Duration::from_secs(10)
            })
        );
    }

    #[test]
    fn unbounded_policy_never_blocks_and_reports_zero_usage() {
        let budget = DeviceWriteBudget::new(WriteBudgetPolicy::unbounded());
        let t0 = Instant::now();
        for _ in 0..500 {
            assert!(budget.try_acquire(t0, WritePriority::Normal).is_ok());
        }
        assert!(budget
            .check_batch_capacity(t0, 1000, WritePriority::Normal)
            .is_ok());
        assert_eq!(budget.next_ready_at(t0, WritePriority::Normal), None);
        let snap = budget.snapshot(t0);
        assert_eq!(snap.used_in_window, 0);
        assert_eq!(snap.remaining_in_window, usize::MAX);
    }
}
