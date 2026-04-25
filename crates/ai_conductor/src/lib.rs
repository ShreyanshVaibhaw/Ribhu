//! Ribhu AI Conductor — Phase 2
//!
//! The AI Conductor watches the Context Bus and acts proactively. It is the
//! "brain" that connects all panels through their shared nervous system.
//!
//! Architecture (from GIST.md and Phase_2_AI_Conductor.md):
//!
//! ```text
//! Context Bus Event
//!      ↓
//! ContextWindowManager  (assemble workspace state)
//!      ↓
//! ActionClassifier      (should AI act? what level?)
//!      ↓
//! PreferenceTracker     (adjust level based on history)
//!      ↓
//! SecurityPipeline      (check before execution)
//!      ↓
//! Execute / Notify / Block
//! ```
//!
//! Register via `AiConductor::new(context_bus, project_root, cx)`.

pub mod autonomy;
pub mod context_manager;
pub mod preferences;
pub mod proactive;
pub mod security;

use autonomy::{ActionClassifier, ActionOutcome, AiAction, AutonomyLevel};
use context_bus::{ContextBus, ContextEventEnvelope};
use context_manager::ContextWindowManager;
use gpui::{AppContext as _, Context, Entity, EventEmitter, Subscription};
use preferences::{PreferenceTracker, PromotionOffer};
use proactive::ProactiveSuggestionEngine;
use security::{CustomRules, SecurityPipeline, SecurityResult};
use std::path::PathBuf;

// ── Events emitted by AiConductor ────────────────────────────────────────────

/// Events the AiConductor emits so the UI layer can react.
pub enum ConductorEvent {
    /// A Level 2 (Suggest) toast should be shown to the user.
    ShowToast(AiAction),
    /// A Level 3 (Ask) modal should be shown to the user.
    ShowModal(AiAction),
    /// An action was executed automatically (Level 1).
    AutoExecuted(AiAction),
    /// An action was blocked by the security layer.
    ActionBlocked { action_type: String, reason: String },
    /// The developer accepted 10 suggestions of the same type;
    /// offer to promote to Auto.
    PromotionAvailable(PromotionOffer),
    /// Workspace context changed — Thread should refresh its context string.
    ContextUpdated,
}

impl EventEmitter<ConductorEvent> for AiConductor {}

// ── AiConductor Entity ────────────────────────────────────────────────────────

/// The main AI Conductor entity. Create one per workspace.
pub struct AiConductor {
    pub context_manager: ContextWindowManager,
    preferences: PreferenceTracker,
    proactive: ProactiveSuggestionEngine,
    security: SecurityPipeline,
    _context_bus_subscription: Subscription,
}

impl AiConductor {
    /// Create and register the AI Conductor, subscribing to the given Context Bus.
    ///
    /// The `cx` can be any `Context<T>` — call this from `Workspace::new` or any
    /// parent entity's constructor.
    pub fn create<T: 'static>(
        context_bus: Entity<ContextBus>,
        project_root: PathBuf,
        cx: &mut Context<T>,
    ) -> Entity<Self> {
        cx.new(|cx: &mut Context<AiConductor>| {
            let subscription = cx.subscribe(
                &context_bus,
                |conductor: &mut AiConductor, _bus, event: &ContextEventEnvelope, cx| {
                    conductor.handle_event(event, cx);
                },
            );

            AiConductor {
                context_manager: ContextWindowManager::new(),
                preferences: PreferenceTracker::new(),
                proactive: ProactiveSuggestionEngine::new(),
                security: SecurityPipeline::new(project_root, CustomRules::default()),
                _context_bus_subscription: subscription,
            }
        })
    }

    // ── Event Processing ──────────────────────────────────────────────────────

    fn handle_event(&mut self, envelope: &ContextEventEnvelope, cx: &mut Context<Self>) {
        // 1. Update context window
        self.context_manager.handle_event(envelope);

        // Notify Thread that context has changed
        cx.emit(ConductorEvent::ContextUpdated);

        // 2. Classify the event — does it warrant an AI action?
        let Some(action) = ActionClassifier::classify(envelope) else {
            self.maybe_run_proactive_check(cx);
            return;
        };

        // 3. Check preferences (may suppress via cooldown or promote to Auto)
        let Some(level) = self.preferences.get_effective_level(&action.action_type) else {
            log::debug!(
                target: "ribhu::ai_conductor",
                "[Conductor] suppressed '{}' (cooldown)", action.action_type
            );
            return;
        };

        let action = AiAction { level, ..action };

        // 4. Route by autonomy level
        match action.level {
            AutonomyLevel::Auto => {
                cx.emit(ConductorEvent::AutoExecuted(action));
            }
            AutonomyLevel::Suggest => {
                cx.emit(ConductorEvent::ShowToast(action));
            }
            AutonomyLevel::Ask => {
                cx.emit(ConductorEvent::ShowModal(action));
            }
        }

        self.maybe_run_proactive_check(cx);
    }

    fn maybe_run_proactive_check(&mut self, cx: &mut Context<Self>) {
        if !self.proactive.should_check() {
            return;
        }
        let suggestions = self.proactive.check(&self.context_manager);
        for suggestion in suggestions {
            let effective = self.preferences.get_effective_level(&suggestion.action_type);
            if let Some(level) = effective {
                let suggestion = AiAction { level, ..suggestion };
                match suggestion.level {
                    AutonomyLevel::Suggest => cx.emit(ConductorEvent::ShowToast(suggestion)),
                    AutonomyLevel::Ask => cx.emit(ConductorEvent::ShowModal(suggestion)),
                    AutonomyLevel::Auto => cx.emit(ConductorEvent::AutoExecuted(suggestion)),
                }
            }
        }
    }

    // ── User Response Handlers ────────────────────────────────────────────────

    /// Call when the user accepts a suggestion.
    pub fn on_accept(&mut self, action: &AiAction, cx: &mut Context<Self>) {
        if let Some(offer) = self.preferences.record_outcome(&action.action_type, ActionOutcome::Accepted) {
            cx.emit(ConductorEvent::PromotionAvailable(offer));
        }
    }

    /// Call when the user dismisses a suggestion.
    pub fn on_dismiss(&mut self, action: &AiAction, _cx: &mut Context<Self>) {
        self.preferences.record_outcome(&action.action_type, ActionOutcome::Dismissed);
    }

    /// Run the security pipeline before executing an action.
    pub fn check_security(&mut self, action: &AiAction) -> SecurityCheckOutcome {
        let Some(ref suggested) = action.suggested_action else {
            return SecurityCheckOutcome::Allow;
        };
        match self.security.check(&action.action_type, suggested) {
            SecurityResult::Allow => SecurityCheckOutcome::Allow,
            SecurityResult::Block { reason, .. } => SecurityCheckOutcome::Block(reason),
            SecurityResult::Ask { reason, .. } => SecurityCheckOutcome::EscalateToModal(reason),
        }
    }

    // ── Context Access ────────────────────────────────────────────────────────

    /// Returns the current workspace context markdown for the AI system prompt.
    pub fn prompt_context(&self) -> String {
        self.context_manager.assemble_prompt_context()
    }

    /// Audit stats for the status bar shield indicator: (blocked, approved).
    pub fn audit_stats(&self) -> (usize, usize) {
        (self.security.audit.blocked_count(), self.security.audit.approved_count())
    }
}

/// Outcome of a security check on a pending action.
#[derive(Debug, Clone)]
pub enum SecurityCheckOutcome {
    Allow,
    Block(String),
    EscalateToModal(String),
}
