use crate::{
    AiSurfaceKind, ContextBus, ContextBusConfig, ContextEvent, ContextEventEnvelope,
    ContextMiddleware, ContextSource, ErrorSeverity, Priority,
};
use gpui::{AppContext as _, Context, Entity, Subscription, TestAppContext};
use std::{
    cell::RefCell,
    rc::Rc,
    time::{Duration, Instant},
};

// ── Helpers ────────────────────────────────────────────────

struct EventRecorder {
    _subscription: Subscription,
}

impl EventRecorder {
    fn new(
        bus: Entity<ContextBus>,
        events: Rc<RefCell<Vec<ContextEventEnvelope>>>,
        cx: &mut Context<Self>,
    ) -> Self {
        let subscription = cx.subscribe(&bus, move |_, _, event: &ContextEventEnvelope, _| {
            events.borrow_mut().push(event.clone());
        });
        Self {
            _subscription: subscription,
        }
    }
}

// ── 1. Publish and receive ─────────────────────────────────

#[gpui::test]
async fn publish_emits_to_subscribers(cx: &mut TestAppContext) {
    let bus = cx.update(|cx| cx.new(|_| ContextBus::default()));
    let events = Rc::new(RefCell::new(Vec::new()));
    let _recorder = cx.update(|cx| cx.new(|cx| EventRecorder::new(bus.clone(), events.clone(), cx)));

    cx.update(|cx| {
        bus.update(cx, |bus: &mut ContextBus, cx| {
            bus.publish(
                ContextSource::Editor,
                Some(7),
                ContextEvent::FileSaved {
                    path: Some("C:/work/app.rs".into()),
                    language: None,
                    lines_changed: None,
                    diagnostics_count: None,
                },
                cx,
            );
        });
    });

    cx.run_until_parked();

    let events = events.borrow();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].metadata.workspace_id, Some(7));
    assert_eq!(events[0].summary(), "Saved app.rs");
}

// ── 2. Multiple subscribers ────────────────────────────────

#[gpui::test]
async fn multiple_subscribers_all_receive_event(cx: &mut TestAppContext) {
    let bus = cx.update(|cx| cx.new(|_| ContextBus::default()));
    let events_a = Rc::new(RefCell::new(Vec::new()));
    let events_b = Rc::new(RefCell::new(Vec::new()));
    let events_c = Rc::new(RefCell::new(Vec::new()));

    let _ra = cx.update(|cx| cx.new(|cx| EventRecorder::new(bus.clone(), events_a.clone(), cx)));
    let _rb = cx.update(|cx| cx.new(|cx| EventRecorder::new(bus.clone(), events_b.clone(), cx)));
    let _rc = cx.update(|cx| cx.new(|cx| EventRecorder::new(bus.clone(), events_c.clone(), cx)));

    cx.update(|cx| {
        bus.update(cx, |bus, cx| {
            bus.publish(
                ContextSource::Git,
                None,
                ContextEvent::BranchChanged {
                    branch: "main".into(),
                },
                cx,
            );
        });
    });

    cx.run_until_parked();

    assert_eq!(events_a.borrow().len(), 1);
    assert_eq!(events_b.borrow().len(), 1);
    assert_eq!(events_c.borrow().len(), 1);
}

// ── 3. Replay buffer returns stored events ─────────────────

#[test]
fn replay_buffer_stores_events() {
    let mut middleware = ContextMiddleware::new(ContextBusConfig {
        replay_capacity: 100,
        dedup_window: Duration::from_millis(0), // disable dedup for this test
        ..ContextBusConfig::default()
    });
    let start = Instant::now();

    for i in 0..5 {
        middleware.publish_with_clock(
            ContextSource::Terminal,
            Some(1),
            ContextEvent::CmdExecuted {
                command: Some(format!("cmd_{i}")),
                exit_code: Some(0),
                summary: format!("ok {i}"),
            },
            start + Duration::from_secs(i as u64 + 1),
            i as u64 + 1,
        );
    }

    let replay = middleware.replay();
    assert_eq!(replay.len(), 5);
}

// ── 4. Replay buffer bounded by capacity ───────────────────

#[test]
fn replay_buffer_is_bounded() {
    let mut middleware = ContextMiddleware::new(ContextBusConfig {
        replay_capacity: 2,
        ..ContextBusConfig::default()
    });
    let start = Instant::now();

    middleware.publish_with_clock(
        ContextSource::Git,
        Some(1),
        ContextEvent::BranchChanged {
            branch: "main".into(),
        },
        start,
        1,
    );
    middleware.publish_with_clock(
        ContextSource::Git,
        Some(1),
        ContextEvent::BranchChanged {
            branch: "feature-a".into(),
        },
        start + Duration::from_secs(2),
        2,
    );
    middleware.publish_with_clock(
        ContextSource::Git,
        Some(1),
        ContextEvent::BranchChanged {
            branch: "feature-b".into(),
        },
        start + Duration::from_secs(4),
        3,
    );

    let replay = middleware.replay();
    assert_eq!(replay.len(), 2);
    assert_eq!(
        replay[0].event,
        ContextEvent::BranchChanged {
            branch: "feature-a".into()
        }
    );
    assert_eq!(
        replay[1].event,
        ContextEvent::BranchChanged {
            branch: "feature-b".into()
        }
    );
}

// ── 5. Dedup within window ─────────────────────────────────

#[test]
fn duplicate_events_are_suppressed_inside_the_dedup_window() {
    let mut middleware = ContextMiddleware::new(ContextBusConfig {
        dedup_window: Duration::from_secs(2),
        ..ContextBusConfig::default()
    });
    let start = Instant::now();

    let first = middleware.publish_with_clock(
        ContextSource::AgentUi,
        Some(9),
        ContextEvent::AiToolSurfaced {
            kind: AiSurfaceKind::Diff,
        },
        start,
        10,
    );
    let duplicate = middleware.publish_with_clock(
        ContextSource::AgentUi,
        Some(9),
        ContextEvent::AiToolSurfaced {
            kind: AiSurfaceKind::Diff,
        },
        start + Duration::from_millis(500),
        11,
    );
    let after_window = middleware.publish_with_clock(
        ContextSource::AgentUi,
        Some(9),
        ContextEvent::AiToolSurfaced {
            kind: AiSurfaceKind::Diff,
        },
        start + Duration::from_secs(3),
        12,
    );

    assert!(first.is_some());
    assert!(duplicate.is_none());
    assert!(after_window.is_some());
    assert_eq!(middleware.stats().deduplicated, 1);
}

// ── 6. Dedup: different sources both pass ──────────────────

#[test]
fn dedup_different_sources_both_pass() {
    let mut middleware = ContextMiddleware::new(ContextBusConfig {
        dedup_window: Duration::from_secs(2),
        ..ContextBusConfig::default()
    });
    let start = Instant::now();

    let from_editor = middleware.publish_with_clock(
        ContextSource::Editor,
        Some(1),
        ContextEvent::FileSaved {
            path: Some("app.rs".into()),
            language: None,
            lines_changed: None,
            diagnostics_count: None,
        },
        start,
        1,
    );
    let from_terminal = middleware.publish_with_clock(
        ContextSource::Terminal,
        Some(1),
        ContextEvent::FileSaved {
            path: Some("app.rs".into()),
            language: None,
            lines_changed: None,
            diagnostics_count: None,
        },
        start + Duration::from_millis(50),
        2,
    );

    assert!(from_editor.is_some());
    assert!(from_terminal.is_some());
    assert_eq!(middleware.stats().deduplicated, 0);
}

// ── 7. Never dedup errors ──────────────────────────────────

#[test]
fn dedup_never_drops_errors() {
    let mut middleware = ContextMiddleware::new(ContextBusConfig {
        dedup_window: Duration::from_secs(5),
        ..ContextBusConfig::default()
    });
    let start = Instant::now();

    let error_event = || ContextEvent::ErrorDetected {
        message: "segfault".into(),
        file_path: None,
        line: None,
        column: None,
        severity: ErrorSeverity::Error,
        command: None,
        exit_code: Some(139),
    };

    let first = middleware.publish_with_clock(
        ContextSource::Terminal,
        Some(1),
        error_event(),
        start,
        1,
    );
    let second = middleware.publish_with_clock(
        ContextSource::Terminal,
        Some(1),
        error_event(),
        start + Duration::from_millis(100),
        2,
    );

    assert!(first.is_some());
    assert!(second.is_some(), "ErrorDetected should never be deduplicated");
    assert_eq!(middleware.stats().deduplicated, 0);
}

// ── 8. Never dedup test failures ───────────────────────────

#[test]
fn dedup_never_drops_test_failures() {
    let mut middleware = ContextMiddleware::new(ContextBusConfig {
        dedup_window: Duration::from_secs(5),
        ..ContextBusConfig::default()
    });
    let start = Instant::now();

    let failure = || ContextEvent::TestFailed {
        test_name: "test_login".into(),
        suite: "auth".into(),
        error: "assertion failed".into(),
        file_path: None,
        line: None,
    };

    let first = middleware.publish_with_clock(
        ContextSource::TestRunner,
        Some(1),
        failure(),
        start,
        1,
    );
    let second = middleware.publish_with_clock(
        ContextSource::TestRunner,
        Some(1),
        failure(),
        start + Duration::from_millis(50),
        2,
    );

    assert!(first.is_some());
    assert!(second.is_some(), "TestFailed should never be deduplicated");
}

// ── 9. Cursor rate limiting ────────────────────────────────

#[test]
fn cursor_events_are_rate_limited() {
    let mut middleware = ContextMiddleware::new(ContextBusConfig {
        cursor_rate_limit: Duration::from_millis(100),
        ..ContextBusConfig::default()
    });
    let start = Instant::now();

    let first = middleware.publish_with_clock(
        ContextSource::Editor,
        Some(11),
        ContextEvent::CursorMoved {
            path: Some("C:/work/app.rs".into()),
            row: 3,
            column: 8,
            symbol_name: None,
        },
        start,
        1,
    );
    let rate_limited = middleware.publish_with_clock(
        ContextSource::Editor,
        Some(11),
        ContextEvent::CursorMoved {
            path: Some("C:/work/app.rs".into()),
            row: 3,
            column: 9,
            symbol_name: None,
        },
        start + Duration::from_millis(40),
        2,
    );
    let accepted_after_window = middleware.publish_with_clock(
        ContextSource::Editor,
        Some(11),
        ContextEvent::CursorMoved {
            path: Some("C:/work/app.rs".into()),
            row: 4,
            column: 0,
            symbol_name: None,
        },
        start + Duration::from_millis(150),
        3,
    );

    assert!(first.is_some());
    assert!(rate_limited.is_none());
    assert!(accepted_after_window.is_some());
    assert_eq!(middleware.stats().rate_limited, 1);
}

// ── 10. Priority classifier ────────────────────────────────

#[test]
fn priority_classifier_assigns_correct_levels() {
    let mut middleware = ContextMiddleware::default();
    let start = Instant::now();

    let error = middleware
        .publish_with_clock(
            ContextSource::Terminal,
            Some(1),
            ContextEvent::ErrorDetected {
                message: "crash".into(),
                file_path: None,
                line: None,
                column: None,
                severity: ErrorSeverity::Error,
                command: None,
                exit_code: Some(1),
            },
            start,
            1,
        )
        .unwrap();
    assert_eq!(error.metadata.priority, Priority::High);

    let cursor = middleware
        .publish_with_clock(
            ContextSource::Editor,
            Some(1),
            ContextEvent::CursorMoved {
                path: Some("app.rs".into()),
                row: 1,
                column: 0,
                symbol_name: None,
            },
            start + Duration::from_secs(1),
            2,
        )
        .unwrap();
    assert_eq!(cursor.metadata.priority, Priority::Low);

    let save = middleware
        .publish_with_clock(
            ContextSource::Editor,
            Some(1),
            ContextEvent::FileSaved {
                path: Some("app.rs".into()),
                language: None,
                lines_changed: None,
                diagnostics_count: None,
            },
            start + Duration::from_secs(2),
            3,
        )
        .unwrap();
    assert_eq!(save.metadata.priority, Priority::Normal);
}

// ── 11. Event serialization (summary round-trips) ──────────

#[test]
fn every_event_variant_has_a_summary() {
    let events: Vec<ContextEvent> = vec![
        ContextEvent::FileOpened { path: Some("a.rs".into()), language: None, line_count: None },
        ContextEvent::FileSaved { path: Some("a.rs".into()), language: None, lines_changed: None, diagnostics_count: None },
        ContextEvent::FileCreated { path: "b.rs".into() },
        ContextEvent::CursorMoved { path: Some("a.rs".into()), row: 0, column: 0, symbol_name: None },
        ContextEvent::CmdExecuted { command: Some("cargo build".into()), exit_code: Some(0), summary: "ok".into() },
        ContextEvent::ErrorDetected { message: "err".into(), file_path: None, line: None, column: None, severity: ErrorSeverity::Error, command: None, exit_code: None },
        ContextEvent::OutputLogged { lines: vec!["hello".into()] },
        ContextEvent::TestPassed { test_name: "t".into(), suite: "s".into(), duration_ms: 10 },
        ContextEvent::TestFailed { test_name: "t".into(), suite: "s".into(), error: "e".into(), file_path: None, line: None },
        ContextEvent::BranchChanged { branch: "main".into() },
        ContextEvent::CommitMade { sha: "abc1234".into(), summary: Some("init".into()) },
        ContextEvent::ConflictFound { files: vec!["a.rs".into()] },
        ContextEvent::ApiRequest { method: "GET".into(), url: "/api".into() },
        ContextEvent::ApiResponse { status: 200, url: "/api".into(), duration_ms: 50 },
        ContextEvent::DbQuery { sql: "SELECT 1".into(), row_count: Some(1), duration_ms: 5 },
        ContextEvent::AiToolSurfaced { kind: AiSurfaceKind::Diff },
        ContextEvent::AiSuggestion { action: "fix".into(), confidence_pct: 90, description: "fix the bug".into() },
        ContextEvent::ProjectSwitched { from_project: None, to_project: "ribhu".into() },
    ];

    for event in &events {
        let summary = event.summary();
        assert!(!summary.is_empty(), "summary for {:?} should not be empty", event.type_name());
    }
    assert_eq!(events.len(), 18, "should cover all 18 event variants");
}

// ── 12. Stats tracking ─────────────────────────────────────

#[test]
fn stats_count_published_deduped_and_rate_limited() {
    let mut middleware = ContextMiddleware::new(ContextBusConfig {
        dedup_window: Duration::from_secs(2),
        cursor_rate_limit: Duration::from_millis(200),
        ..ContextBusConfig::default()
    });
    let start = Instant::now();

    // 1 published
    middleware.publish_with_clock(
        ContextSource::Editor,
        Some(1),
        ContextEvent::FileSaved { path: None, language: None, lines_changed: None, diagnostics_count: None },
        start,
        1,
    );
    // 1 deduped
    middleware.publish_with_clock(
        ContextSource::Editor,
        Some(1),
        ContextEvent::FileSaved { path: None, language: None, lines_changed: None, diagnostics_count: None },
        start + Duration::from_millis(100),
        2,
    );
    // 1 published (cursor)
    middleware.publish_with_clock(
        ContextSource::Editor,
        Some(1),
        ContextEvent::CursorMoved { path: Some("a.rs".into()), row: 0, column: 0, symbol_name: None },
        start + Duration::from_millis(200),
        3,
    );
    // 1 rate-limited (cursor within window)
    middleware.publish_with_clock(
        ContextSource::Editor,
        Some(1),
        ContextEvent::CursorMoved { path: Some("a.rs".into()), row: 0, column: 1, symbol_name: None },
        start + Duration::from_millis(250),
        4,
    );

    let stats = middleware.stats();
    assert_eq!(stats.published, 2);
    assert_eq!(stats.deduplicated, 1);
    assert_eq!(stats.rate_limited, 1);
}
