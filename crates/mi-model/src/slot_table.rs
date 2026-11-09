use crate::navigation_history::NavigationPlace;
use serde::{Deserialize, Serialize};

pub const SLOT_COUNT: usize = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SlotMode {
    Pinned,
    Recent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SlotStatus {
    Idle,
    Running,
    NeedsInteraction,
    Unread,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlotEntry {
    pub slot_index: usize,
    pub mode: SlotMode,
    pub place: Option<NavigationPlace>,
    pub title: String,
    pub status: SlotStatus,
    pub pinned_at: Option<u64>,
    pub updated_at: Option<u64>,
}

impl SlotEntry {
    pub fn empty(slot_index: usize) -> Self {
        Self {
            slot_index,
            mode: SlotMode::Recent,
            place: None,
            title: String::new(),
            status: SlotStatus::Idle,
            pinned_at: None,
            updated_at: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecentCandidate {
    pub place: NavigationPlace,
    pub title: String,
    pub status: SlotStatus,
    pub updated_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlotError {
    IndexOutOfBounds(usize),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlotTable {
    slots: [SlotEntry; SLOT_COUNT],
}

impl Default for SlotTable {
    fn default() -> Self {
        Self::new()
    }
}

impl SlotTable {
    pub fn new() -> Self {
        Self {
            slots: [
                SlotEntry::empty(0),
                SlotEntry::empty(1),
                SlotEntry::empty(2),
                SlotEntry::empty(3),
                SlotEntry::empty(4),
                SlotEntry::empty(5),
            ],
        }
    }

    pub fn slots(&self) -> &[SlotEntry; SLOT_COUNT] {
        &self.slots
    }

    pub fn slots_mut(&mut self) -> &mut [SlotEntry; SLOT_COUNT] {
        &mut self.slots
    }

    pub fn get(&self, slot_index: usize) -> Option<&SlotEntry> {
        self.slots.get(slot_index)
    }

    pub fn is_pinned(&self, slot_index: usize) -> bool {
        self.slots
            .get(slot_index)
            .map_or(false, |s| s.mode == SlotMode::Pinned)
    }

    pub fn active_place(&self, slot_index: usize) -> Option<&NavigationPlace> {
        self.slots.get(slot_index).and_then(|s| s.place.as_ref())
    }

    pub fn pin(
        &mut self,
        slot_index: usize,
        place: NavigationPlace,
        title: impl Into<String>,
        timestamp: u64,
    ) -> Result<(), SlotError> {
        if slot_index >= SLOT_COUNT {
            return Err(SlotError::IndexOutOfBounds(slot_index));
        }
        self.slots[slot_index] = SlotEntry {
            slot_index,
            mode: SlotMode::Pinned,
            place: Some(place),
            title: title.into(),
            status: SlotStatus::Idle,
            pinned_at: Some(timestamp),
            updated_at: Some(timestamp),
        };
        Ok(())
    }

    pub fn unpin(&mut self, slot_index: usize) -> Result<(), SlotError> {
        if slot_index >= SLOT_COUNT {
            return Err(SlotError::IndexOutOfBounds(slot_index));
        }
        self.slots[slot_index].mode = SlotMode::Recent;
        self.slots[slot_index].pinned_at = None;
        Ok(())
    }

    /// Fills unpinned slots with the most recent candidates that are NOT already pinned in any slot.
    /// Preserves pinned slots unchanged.
    pub fn sync_recent(&mut self, candidates: &[RecentCandidate]) {
        let pinned_places: std::collections::HashSet<String> = self
            .slots
            .iter()
            .filter(|s| s.mode == SlotMode::Pinned)
            .filter_map(|s| s.place.as_ref().map(|p| p.target_id()))
            .collect();

        let mut available_candidates = candidates
            .iter()
            .filter(|c| !pinned_places.contains(&c.place.target_id()))
            .cloned();

        for slot in self.slots.iter_mut() {
            if slot.mode == SlotMode::Pinned {
                continue;
            }
            if let Some(candidate) = available_candidates.next() {
                slot.place = Some(candidate.place);
                slot.title = candidate.title;
                slot.status = candidate.status;
                slot.updated_at = Some(candidate.updated_at);
            } else {
                slot.place = None;
                slot.title.clear();
                slot.status = SlotStatus::Idle;
                slot.updated_at = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn six_slots_initialize_as_recent_and_empty() {
        let table = SlotTable::new();
        assert_eq!(table.slots().len(), 6);
        for (i, slot) in table.slots().iter().enumerate() {
            assert_eq!(slot.slot_index, i);
            assert_eq!(slot.mode, SlotMode::Recent);
            assert_eq!(slot.place, None);
            assert!(slot.title.is_empty());
        }
    }

    #[test]
    fn pin_and_unpin_slot_manages_mode_and_preserves_slot_index() {
        let mut table = SlotTable::new();
        let target = NavigationPlace::antigravity("c1", "/qa/workspace");
        table.pin(2, target.clone(), "Alpha Task", 100).unwrap();

        assert!(table.is_pinned(2));
        assert_eq!(table.get(2).unwrap().mode, SlotMode::Pinned);
        assert_eq!(table.active_place(2), Some(&target));
        assert_eq!(table.get(2).unwrap().title, "Alpha Task");
        assert_eq!(table.get(2).unwrap().pinned_at, Some(100));

        table.unpin(2).unwrap();
        assert!(!table.is_pinned(2));
        assert_eq!(table.get(2).unwrap().mode, SlotMode::Recent);
        assert_eq!(table.get(2).unwrap().pinned_at, None);
        assert_eq!(table.active_place(2), Some(&target));
    }

    #[test]
    fn sync_recent_populates_unpinned_slots_without_displacing_pinned_slots() {
        let mut table = SlotTable::new();
        let pinned_target = NavigationPlace::cindy("/ws", "sess-pinned");
        table
            .pin(0, pinned_target.clone(), "Pinned Task", 500)
            .unwrap();

        let c1 = RecentCandidate {
            place: NavigationPlace::antigravity("ag-0", "/ws"),
            title: "AG Task 1".into(),
            status: SlotStatus::Running,
            updated_at: 1000,
        };
        let c2 = RecentCandidate {
            place: NavigationPlace::dsh("dsh-1", "ws-1", "host-1"),
            title: "DSH Task 2".into(),
            status: SlotStatus::NeedsInteraction,
            updated_at: 900,
        };
        // Candidate that matches the pinned target must not duplicate into recent slots
        let c3 = RecentCandidate {
            place: pinned_target.clone(),
            title: "Pinned Duplicate".into(),
            status: SlotStatus::Idle,
            updated_at: 800,
        };
        let c4 = RecentCandidate {
            place: NavigationPlace::antigravity("ag-1", "/ws"),
            title: "AG Task 4".into(),
            status: SlotStatus::Unread,
            updated_at: 700,
        };

        table.sync_recent(&[c1.clone(), c2.clone(), c3, c4.clone()]);

        // Slot 0 remains pinned
        assert!(table.is_pinned(0));
        assert_eq!(table.active_place(0), Some(&pinned_target));
        assert_eq!(table.get(0).unwrap().title, "Pinned Task");

        // Slot 1 gets c1
        assert_eq!(table.get(1).unwrap().mode, SlotMode::Recent);
        assert_eq!(table.active_place(1), Some(&c1.place));
        assert_eq!(table.get(1).unwrap().title, "AG Task 1");
        assert_eq!(table.get(1).unwrap().status, SlotStatus::Running);

        // Slot 2 gets c2
        assert_eq!(table.active_place(2), Some(&c2.place));
        assert_eq!(table.get(2).unwrap().status, SlotStatus::NeedsInteraction);

        // Slot 3 gets c4 (c3 was skipped because it's pinned in slot 0)
        assert_eq!(table.active_place(3), Some(&c4.place));

        // Slots 4 and 5 are empty
        assert_eq!(table.active_place(4), None);
        assert_eq!(table.active_place(5), None);
    }
}
