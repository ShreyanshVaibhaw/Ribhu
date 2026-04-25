use crate::{
    events::{ContextEvent, ContextEventEnvelope, ContextSource},
    middleware::{ContextBusConfig, ContextBusStats, ContextMiddleware},
};
use gpui::{Context, EventEmitter};

pub struct ContextBus {
    middleware: ContextMiddleware,
}

impl ContextBus {
    pub fn new(config: ContextBusConfig) -> Self {
        Self {
            middleware: ContextMiddleware::new(config),
        }
    }

    pub fn publish(
        &mut self,
        source: ContextSource,
        workspace_id: Option<i64>,
        event: ContextEvent,
        cx: &mut Context<Self>,
    ) -> Option<ContextEventEnvelope> {
        let envelope = self.middleware.publish(source, workspace_id, event);
        if let Some(envelope) = &envelope {
            log::debug!(
                target: "ribhu::context_bus",
                "[CONTEXT BUS] {} → {} (priority: {:?})",
                envelope.metadata.source.as_str(),
                envelope.event.type_name(),
                envelope.metadata.priority
            );
            cx.emit(envelope.clone());
        }
        envelope
    }

    pub fn replay(&self) -> Vec<ContextEventEnvelope> {
        self.middleware.replay()
    }

    pub fn latest(&self) -> Option<ContextEventEnvelope> {
        self.middleware.latest().cloned()
    }

    pub fn stats(&self) -> ContextBusStats {
        self.middleware.stats()
    }
}

impl Default for ContextBus {
    fn default() -> Self {
        Self::new(ContextBusConfig::default())
    }
}

impl EventEmitter<ContextEventEnvelope> for ContextBus {}
