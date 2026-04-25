use crate::events::{ContextEvent, ContextEventEnvelope, ContextEventMetadata, ContextSource};
use std::{
    collections::{HashMap, VecDeque},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContextBusConfig {
    pub replay_capacity: usize,
    pub dedup_window: Duration,
    pub cursor_rate_limit: Duration,
}

impl Default for ContextBusConfig {
    fn default() -> Self {
        Self {
            replay_capacity: 256,
            dedup_window: Duration::from_millis(1_500),
            cursor_rate_limit: Duration::from_millis(120),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ContextBusStats {
    pub published: usize,
    pub deduplicated: usize,
    pub rate_limited: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct DedupKey {
    workspace_id: Option<i64>,
    source: ContextSource,
    event: ContextEvent,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct CursorKey {
    workspace_id: Option<i64>,
    source: ContextSource,
    path: Option<String>,
}

pub struct ContextMiddleware {
    config: ContextBusConfig,
    replay: VecDeque<ContextEventEnvelope>,
    dedup_cache: HashMap<DedupKey, Instant>,
    cursor_cache: HashMap<CursorKey, Instant>,
    stats: ContextBusStats,
}

impl ContextMiddleware {
    pub fn new(config: ContextBusConfig) -> Self {
        Self {
            config,
            replay: VecDeque::with_capacity(config.replay_capacity),
            dedup_cache: HashMap::default(),
            cursor_cache: HashMap::default(),
            stats: ContextBusStats::default(),
        }
    }

    pub fn replay(&self) -> Vec<ContextEventEnvelope> {
        self.replay.iter().cloned().collect()
    }

    pub fn latest(&self) -> Option<&ContextEventEnvelope> {
        self.replay.back()
    }

    pub fn stats(&self) -> ContextBusStats {
        self.stats
    }

    pub(crate) fn publish(
        &mut self,
        source: ContextSource,
        workspace_id: Option<i64>,
        event: ContextEvent,
    ) -> Option<ContextEventEnvelope> {
        self.publish_with_clock(
            source,
            workspace_id,
            event,
            Instant::now(),
            current_timestamp_ms(),
        )
    }

    pub(crate) fn publish_with_clock(
        &mut self,
        source: ContextSource,
        workspace_id: Option<i64>,
        event: ContextEvent,
        now: Instant,
        timestamp_ms: u64,
    ) -> Option<ContextEventEnvelope> {
        self.prune_caches(now);

        // Rate-limit high-frequency cursor events.
        if let Some(cursor_key) = cursor_key(source, workspace_id, &event) {
            if self
                .cursor_cache
                .get(&cursor_key)
                .is_some_and(|last_seen| now.duration_since(*last_seen) < self.config.cursor_rate_limit)
            {
                self.stats.rate_limited += 1;
                return None;
            }

            self.cursor_cache.insert(cursor_key, now);
        }

        // Dedup: skip if the same event was published within the window.
        // NEVER dedup errors, test failures, conflicts, or AI suggestions.
        if !event.never_dedup() {
            let dedup_key = DedupKey {
                workspace_id,
                source,
                event: event.clone(),
            };
            if self
                .dedup_cache
                .get(&dedup_key)
                .is_some_and(|last_seen| now.duration_since(*last_seen) < self.config.dedup_window)
            {
                self.stats.deduplicated += 1;
                return None;
            }
            self.dedup_cache.insert(dedup_key, now);
        }

        // Auto-classify priority based on event type.
        let priority = event.default_priority();

        let envelope = ContextEventEnvelope {
            metadata: ContextEventMetadata {
                workspace_id,
                source,
                timestamp_ms,
                priority,
            },
            event,
        };

        if self.config.replay_capacity > 0 {
            if self.replay.len() == self.config.replay_capacity {
                self.replay.pop_front();
            }
            self.replay.push_back(envelope.clone());
        }

        self.stats.published += 1;
        Some(envelope)
    }

    fn prune_caches(&mut self, now: Instant) {
        let dedup_window = self.config.dedup_window;
        self.dedup_cache
            .retain(|_, last_seen| now.duration_since(*last_seen) <= dedup_window);

        let cursor_rate_limit = self.config.cursor_rate_limit;
        self.cursor_cache
            .retain(|_, last_seen| now.duration_since(*last_seen) <= cursor_rate_limit);
    }
}

impl Default for ContextMiddleware {
    fn default() -> Self {
        Self::new(ContextBusConfig::default())
    }
}

fn cursor_key(
    source: ContextSource,
    workspace_id: Option<i64>,
    event: &ContextEvent,
) -> Option<CursorKey> {
    match event {
        ContextEvent::CursorMoved { path, .. } => Some(CursorKey {
            workspace_id,
            source,
            path: path.clone(),
        }),
        _ => None,
    }
}

fn current_timestamp_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
