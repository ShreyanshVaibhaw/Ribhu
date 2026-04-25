//! Workspace-scoped context routing for Ribhu.

pub mod bridge;
pub mod bus;
pub mod events;
pub mod middleware;

pub use bus::ContextBus;
pub use events::{
    AiSurfaceKind, ContextEvent, ContextEventEnvelope, ContextEventMetadata, ContextSource,
    ErrorSeverity, Priority,
};
pub use middleware::{ContextBusConfig, ContextBusStats, ContextMiddleware};

#[cfg(test)]
mod tests;
