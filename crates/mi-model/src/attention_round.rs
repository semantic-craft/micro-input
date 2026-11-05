use crate::navigation_history::NavigationPlace;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttentionCandidate {
    pub place: NavigationPlace,
    pub title: String,
    /// Priority rank:
    /// 0: needs-interaction (pending approval, user question, plan review)
    /// 1: explicit unread (ONLY when unread is Some(true))
    /// 2: error / attention required
    pub priority_rank: usize,
    /// Unique signature of the current event/interaction/unread watermark.
    /// Changes when new events, interactions, or unread states occur.
    pub watermark_signature: String,
    /// True only when backed by an explicit interaction, explicit unread fact, or error attention.
    /// Completed sessions with unread == false or unread == None are never inferred as unread.
    pub is_eligible: bool,
}

impl AttentionCandidate {
    pub fn new(
        place: NavigationPlace,
        title: impl Into<String>,
        priority_rank: usize,
        watermark_signature: impl Into<String>,
        is_eligible: bool,
    ) -> Self {
        Self {
            place,
            title: title.into(),
            priority_rank,
            watermark_signature: watermark_signature.into(),
            is_eligible,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AttentionRound {
    visited_signatures: HashMap<NavigationPlace, String>,
    visited_count: usize,
}

impl AttentionRound {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset(&mut self) {
        self.visited_signatures.clear();
        self.visited_count = 0;
    }

    pub fn visited_count(&self) -> usize {
        self.visited_count
    }

    pub fn is_visited(&self, place: &NavigationPlace, signature: &str) -> bool {
        self.visited_signatures
            .get(place)
            .map_or(false, |s| s == signature)
    }

    /// Advances the attention round.
    ///
    /// Respects the invariant:
    /// - Candidates are visited in priority order (rank 0 < 1 < 2).
    /// - Candidates already visited with the same signature in this round are skipped.
    /// - New events update the candidate's signature, allowing it to re-enter the round.
    /// - Returns None when the round is exhausted (all candidates visited), unless allow_wrap is true.
    pub fn next_candidate(
        &mut self,
        candidates: &[AttentionCandidate],
        allow_wrap: bool,
    ) -> Option<AttentionCandidate> {
        let mut eligible: Vec<AttentionCandidate> = candidates
            .iter()
            .filter(|c| c.is_eligible)
            .cloned()
            .collect();

        if eligible.is_empty() {
            return None;
        }

        // Sort by priority rank first, maintaining order for equal ranks
        eligible.sort_by_key(|c| c.priority_rank);

        // Prune visited signatures for candidates that are no longer present
        let current_places: std::collections::HashSet<_> =
            eligible.iter().map(|c| &c.place).collect();
        self.visited_signatures
            .retain(|p, _| current_places.contains(p));

        // Find first candidate not yet visited with its current signature
        if let Some(candidate) = eligible
            .iter()
            .find(|c| self.visited_signatures.get(&c.place) != Some(&c.watermark_signature))
        {
            self.visited_signatures.insert(
                candidate.place.clone(),
                candidate.watermark_signature.clone(),
            );
            self.visited_count += 1;
            return Some(candidate.clone());
        }

        // Round exhausted
        if allow_wrap {
            self.visited_signatures.clear();
            self.visited_count = 0;
            if let Some(first) = eligible.first() {
                self.visited_signatures
                    .insert(first.place.clone(), first.watermark_signature.clone());
                self.visited_count = 1;
                return Some(first.clone());
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn priority_rank_visits_interaction_before_unread_before_error() {
        let mut round = AttentionRound::new();
        let p_interaction = NavigationPlace::zcode("s", "r", "/ws", "task-perm");
        let p_unread = NavigationPlace::cindy("/ws", "sess-unread");
        let p_error = NavigationPlace::antigravity("ag-err", "/ws");

        let c_error = AttentionCandidate::new(p_error.clone(), "Error Task", 2, "sig-err", true);
        let c_interaction =
            AttentionCandidate::new(p_interaction.clone(), "Perm Task", 0, "sig-perm", true);
        let c_unread =
            AttentionCandidate::new(p_unread.clone(), "Unread Task", 1, "sig-unread", true);

        let list = [c_error, c_interaction, c_unread];

        let first = round.next_candidate(&list, false).unwrap();
        assert_eq!(first.place, p_interaction);

        let second = round.next_candidate(&list, false).unwrap();
        assert_eq!(second.place, p_unread);

        let third = round.next_candidate(&list, false).unwrap();
        assert_eq!(third.place, p_error);

        // Exhausted
        assert_eq!(round.next_candidate(&list, false), None);
    }

    #[test]
    fn new_event_watermark_reenters_attention_round() {
        let mut round = AttentionRound::new();
        let place = NavigationPlace::dsh("dsh-1", "ws", "host-1");
        let c1 = AttentionCandidate::new(place.clone(), "DSH Task", 0, "wm:1:perm1", true);

        let picked = round.next_candidate(&[c1.clone()], false).unwrap();
        assert_eq!(picked.place, place);

        // Without event change, round is exhausted
        assert_eq!(round.next_candidate(&[c1], false), None);

        // New event arrives: watermark changes to wm:2
        let c2 = AttentionCandidate::new(place.clone(), "DSH Task", 0, "wm:2:perm2", true);
        let reentered = round.next_candidate(&[c2.clone()], false).unwrap();
        assert_eq!(reentered.place, place);
        assert_eq!(reentered.watermark_signature, "wm:2:perm2");
    }

    #[test]
    fn unknown_unread_is_never_inferred_for_completed_sessions() {
        let mut round = AttentionRound::new();
        let p_completed = NavigationPlace::antigravity("ag-done", "/ws");
        // Completed session with unknown unread (is_eligible = false)
        let c_completed = AttentionCandidate::new(p_completed, "Done Task", 1, "sig-done", false);

        assert_eq!(round.next_candidate(&[c_completed], false), None);
    }
}
