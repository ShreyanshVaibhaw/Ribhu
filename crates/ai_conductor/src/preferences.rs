//! Preference Tracker — learns from the developer's accept/dismiss responses.
//!
//! Per Phase 2 spec:
//! - After 10 consecutive accepts → offer to promote to Auto
//! - After 3 consecutive dismisses → cooldown (skip next 2 occurrences)
//! - Persists to a simple JSON file alongside the workspace database

use crate::autonomy::{ActionOutcome, AutonomyLevel};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, VecDeque},
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

// ── Constants ────────────────────────────────────────────────────────────────

const PROMOTE_THRESHOLD: u32 = 10;
const COOLDOWN_THRESHOLD: u32 = 3;
const COOLDOWN_SKIP_COUNT: u32 = 2;

// ── Types ────────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ActionHistory {
    total_accepted: u32,
    total_dismissed: u32,
    total_modified: u32,
    /// Ring buffer of the last 10 outcomes for recent-trend analysis.
    last_10: VecDeque<String>,
    /// Effective autonomy level (may have been promoted).
    current_level: String,
    /// Unix timestamp until which this action type is in cooldown.
    cooldown_until_ms: Option<u64>,
    /// How many more occurrences to skip while in cooldown.
    cooldown_skips_remaining: u32,
    /// Whether we've already offered to promote to Auto for this type.
    promotion_offered: bool,
}

impl Default for ActionHistory {
    fn default() -> Self {
        Self {
            total_accepted: 0,
            total_dismissed: 0,
            total_modified: 0,
            last_10: VecDeque::with_capacity(10),
            current_level: "Suggest".into(),
            cooldown_until_ms: None,
            cooldown_skips_remaining: 0,
            promotion_offered: false,
        }
    }
}

// ── Preference Tracker ───────────────────────────────────────────────────────

/// Tracks how the developer responds to AI suggestions and adjusts behavior.
///
/// Persists to a JSON file so preferences survive app restarts.
pub struct PreferenceTracker {
    history: HashMap<String, ActionHistory>,
    db_path: Option<PathBuf>,
}

impl PreferenceTracker {
    pub fn new() -> Self {
        Self {
            history: HashMap::new(),
            db_path: None,
        }
    }

    /// Load from a JSON file, creating it if it doesn't exist.
    pub fn load(path: PathBuf) -> Self {
        let history = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Self {
            history,
            db_path: Some(path),
        }
    }

    /// Persist current state to the JSON file.
    pub fn save(&self) {
        if let Some(ref path) = self.db_path {
            if let Ok(json) = serde_json::to_string_pretty(&self.history) {
                if let Err(error) = std::fs::write(path, json) {
                    log::error!("Failed to save preferences: {error}");
                }
            }
        }
    }

    // ── Core API ─────────────────────────────────────────────────────────────

    /// Record how the user responded to an action. Returns whether a
    /// promotion offer should be shown to the user.
    pub fn record_outcome(
        &mut self,
        action_type: &str,
        outcome: ActionOutcome,
    ) -> Option<PromotionOffer> {
        let history = self.history.entry(action_type.to_string()).or_default();

        let outcome_str = match outcome {
            ActionOutcome::Accepted => "accepted",
            ActionOutcome::Dismissed => "dismissed",
            ActionOutcome::Modified => "modified",
        };

        // Update totals
        match outcome {
            ActionOutcome::Accepted | ActionOutcome::Modified => history.total_accepted += 1,
            ActionOutcome::Dismissed => history.total_dismissed += 1,
        }

        // Maintain last_10 ring buffer
        if history.last_10.len() == 10 {
            history.last_10.pop_front();
        }
        history.last_10.push_back(outcome_str.to_string());

        // Reset cooldown on acceptance
        if matches!(outcome, ActionOutcome::Accepted | ActionOutcome::Modified) {
            history.cooldown_skips_remaining = 0;
            history.cooldown_until_ms = None;
        }

        // Check for cooldown trigger (3 consecutive dismisses)
        let consecutive_dismisses = history
            .last_10
            .iter()
            .rev()
            .take_while(|o| o.as_str() == "dismissed")
            .count();
        if consecutive_dismisses >= COOLDOWN_THRESHOLD as usize {
            history.cooldown_skips_remaining = COOLDOWN_SKIP_COUNT;
            let until = now_ms() + 5 * 60 * 1000; // 5-minute cooldown window
            history.cooldown_until_ms = Some(until);
            log::debug!(
                target: "ribhu::ai_conductor",
                "[Preferences] cooldown triggered for '{}' (3 consecutive dismisses)",
                action_type
            );
        }

        // Check for promotion trigger (10 consecutive accepts)
        let consecutive_accepts = history
            .last_10
            .iter()
            .rev()
            .take_while(|o| o.as_str() == "accepted")
            .count();
        if consecutive_accepts >= PROMOTE_THRESHOLD as usize
            && history.current_level == "Suggest"
            && !history.promotion_offered
        {
            history.promotion_offered = true;
            self.save();
            return Some(PromotionOffer {
                action_type: action_type.to_string(),
                from_level: AutonomyLevel::Suggest,
                to_level: AutonomyLevel::Auto,
            });
        }

        self.save();
        None
    }

    /// Accept a promotion offer — permanently elevate an action type to Auto.
    pub fn accept_promotion(&mut self, action_type: &str) {
        let history = self.history.entry(action_type.to_string()).or_default();
        history.current_level = "Auto".into();
        self.save();
        log::debug!(
            target: "ribhu::ai_conductor",
            "[Preferences] '{}' promoted to Auto", action_type
        );
    }

    /// Decline a promotion offer — keep at Suggest but reset the counter.
    pub fn decline_promotion(&mut self, action_type: &str) {
        let history = self.history.entry(action_type.to_string()).or_default();
        history.promotion_offered = false;
        // Reset last_10 so we don't immediately re-offer
        history.last_10.clear();
        self.save();
    }

    /// Returns the effective autonomy level for an action type, after applying
    /// any promotion or cooldown. Returns `None` if the action is in cooldown
    /// and should be suppressed entirely.
    pub fn get_effective_level(&mut self, action_type: &str) -> Option<AutonomyLevel> {
        let history = self.history.entry(action_type.to_string()).or_default();

        // Check cooldown
        if history.cooldown_skips_remaining > 0 {
            history.cooldown_skips_remaining -= 1;
            log::debug!(
                target: "ribhu::ai_conductor",
                "[Preferences] suppressing '{}' (cooldown, {} skips left)",
                action_type,
                history.cooldown_skips_remaining
            );
            self.save();
            return None; // suppress
        }

        let level = match history.current_level.as_str() {
            "Auto" => AutonomyLevel::Auto,
            "Ask" => AutonomyLevel::Ask,
            _ => AutonomyLevel::Suggest,
        };
        Some(level)
    }
}

impl Default for PreferenceTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Offer shown to the user when a Suggest action has been accepted 10 times.
#[derive(Clone, Debug)]
pub struct PromotionOffer {
    pub action_type: String,
    pub from_level: AutonomyLevel,
    pub to_level: AutonomyLevel,
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_dismisses_triggers_cooldown() {
        let mut tracker = PreferenceTracker::new();
        for _ in 0..3 {
            tracker.record_outcome("fix_test", ActionOutcome::Dismissed);
        }
        // Next call should return None (suppressed)
        assert!(tracker.get_effective_level("fix_test").is_none());
    }

    #[test]
    fn ten_accepts_offers_promotion() {
        let mut tracker = PreferenceTracker::new();
        let mut offer = None;
        for _ in 0..10 {
            offer = tracker.record_outcome("fix_test", ActionOutcome::Accepted);
        }
        assert!(offer.is_some());
        let offer = offer.unwrap();
        assert_eq!(offer.action_type, "fix_test");
        assert_eq!(offer.to_level, AutonomyLevel::Auto);
    }

    #[test]
    fn acceptance_after_promotion_gives_auto_level() {
        let mut tracker = PreferenceTracker::new();
        for _ in 0..10 {
            tracker.record_outcome("fix_test", ActionOutcome::Accepted);
        }
        tracker.accept_promotion("fix_test");
        assert_eq!(
            tracker.get_effective_level("fix_test"),
            Some(AutonomyLevel::Auto)
        );
    }

    #[test]
    fn acceptance_clears_cooldown() {
        let mut tracker = PreferenceTracker::new();
        for _ in 0..3 {
            tracker.record_outcome("fix_test", ActionOutcome::Dismissed);
        }
        // Accept once — should clear cooldown
        tracker.record_outcome("fix_test", ActionOutcome::Accepted);
        // Should be back to Suggest, not suppressed
        assert_eq!(
            tracker.get_effective_level("fix_test"),
            Some(AutonomyLevel::Suggest)
        );
    }
}
