//! Cycle Manager — drives the Scan -> Focus -> Review lifecycle.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use bamboo_core::{
    AgentHeartbeat, AgentRunStatus, BusMessage, ComponentId, CycleConfig, CycleStage,
    CycleStageChanged, CycleSummary, EventBus, InstrumentId, Payload, ResearchFinding, Topic,
};
use uuid::Uuid;

use crate::shutdown::ShutdownSignal;

/// Internal state for the cycle manager.
pub struct CycleState {
    pub current_stage: CycleStage,
    pub cycle_id: Uuid,
    pub focus_set: Vec<InstrumentId>,
    pub must_monitor: HashSet<InstrumentId>,
    pub cycle_count: u32,
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn now_nanos() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64
}

fn bus_msg(topic: Topic, payload: Payload, source: &str) -> BusMessage {
    BusMessage {
        id: Uuid::new_v4(),
        topic,
        payload,
        timestamp: now_nanos(),
        source: ComponentId::new(source),
    }
}

async fn publish_heartbeat(bus: &Arc<dyn EventBus>, status: AgentRunStatus, action: Option<String>) {
    let hb = AgentHeartbeat {
        agent_name: "CycleManager".to_string(),
        status,
        last_action: action,
        timestamp: now_nanos(),
    };
    let msg = bus_msg(Topic::System, Payload::AgentHeartbeat(hb), "CycleManager");
    let _ = bus.publish(msg).await;
}

/// Run the cycle manager as an async task.
///
/// Timing parameters:
/// - `scan_duration`: how long to stay in Scan (default 60s for demo)
/// - `focus_duration`: how long to stay in Focus (remaining cycle time, or default 120s)
/// - `review_duration`: how long Review lasts (default 30s)
pub async fn run_cycle_manager(
    bus: Arc<dyn EventBus>,
    config: CycleConfig,
    max_focus_set: usize,
    shutdown: ShutdownSignal,
) {
    // For demo speed, derive durations from config hours (or use short defaults).
    let total_secs = config.default_duration_hours * 3600;
    let scan_duration = Duration::from_secs(60.min(total_secs / 4));
    let review_duration = Duration::from_secs(30);
    let focus_duration = Duration::from_secs(total_secs.saturating_sub(90).max(120));

    let mut state = CycleState {
        current_stage: CycleStage::Scan,
        cycle_id: Uuid::new_v4(),
        focus_set: Vec::new(),
        must_monitor: HashSet::new(),
        cycle_count: 0,
    };

    tracing::info!("CycleManager started");
    publish_heartbeat(&bus, AgentRunStatus::Running, Some("Starting".to_string())).await;

    // Subscribe to Signal topic to collect ResearchFindings during Scan.
    let mut signal_rx = bus.subscribe(Topic::Signal);
    // Subscribe to Execution topic for PositionUpdate (must_monitor tracking).
    let mut exec_rx = bus.subscribe(Topic::Execution);

    loop {
        if shutdown.is_shutdown() {
            break;
        }

        state.cycle_count += 1;
        state.cycle_id = Uuid::new_v4();

        // ── SCAN ────────────────────────────────────────────────────────
        state.current_stage = CycleStage::Scan;
        tracing::info!(cycle = state.cycle_count, "Entering Scan stage");

        let stage_changed = CycleStageChanged {
            cycle_id: state.cycle_id,
            new_stage: CycleStage::Scan,
            focus_set: state.focus_set.clone(),
            timestamp: now_nanos(),
        };
        let msg = bus_msg(
            Topic::System,
            Payload::CycleStageChanged(stage_changed),
            "CycleManager",
        );
        let _ = bus.publish(msg).await;

        // Collect findings during scan.
        let mut findings: Vec<ResearchFinding> = Vec::new();
        let scan_start = tokio::time::Instant::now();
        let mut heartbeat_interval = tokio::time::interval(Duration::from_secs(10));

        loop {
            if shutdown.is_shutdown() {
                break;
            }
            let remaining = scan_duration.saturating_sub(scan_start.elapsed());
            if remaining.is_zero() {
                break;
            }

            tokio::select! {
                _ = tokio::time::sleep(remaining) => { break; }
                _ = shutdown.wait_for_shutdown() => { break; }
                _ = heartbeat_interval.tick() => {
                    publish_heartbeat(
                        &bus,
                        AgentRunStatus::Running,
                        Some(format!("Scan: collected {} findings", findings.len())),
                    ).await;
                }
                result = signal_rx.recv() => {
                    match result {
                        Ok(msg) => {
                            if let Payload::ResearchFinding(f) = msg.payload {
                                findings.push(f);
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            tracing::warn!(lagged = n, "CycleManager signal_rx lagged");
                        }
                        Err(_) => break,
                    }
                }
                result = exec_rx.recv() => {
                    if let Ok(msg) = result {
                        if let Payload::PositionUpdate(pos) = msg.payload {
                            state.must_monitor.insert(pos.instrument_id);
                        }
                    }
                }
            }
        }

        if shutdown.is_shutdown() {
            break;
        }

        // Build focus set from top findings by score, respecting max_focus_set config.
        findings.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        let mut focus: Vec<InstrumentId> = findings
            .iter()
            .take(max_focus_set)
            .map(|f| f.instrument_id.clone())
            .collect();

        // Merge must_monitor instruments (positions we're tracking) into focus set.
        for inst in &state.must_monitor {
            if !focus.contains(inst) && focus.len() < max_focus_set {
                focus.push(inst.clone());
            }
        }
        state.focus_set = focus;

        tracing::info!(
            cycle = state.cycle_count,
            focus_count = state.focus_set.len(),
            "Scan complete, entering Focus"
        );

        // ── FOCUS ───────────────────────────────────────────────────────
        state.current_stage = CycleStage::Focus;
        let stage_changed = CycleStageChanged {
            cycle_id: state.cycle_id,
            new_stage: CycleStage::Focus,
            focus_set: state.focus_set.clone(),
            timestamp: now_nanos(),
        };
        let msg = bus_msg(
            Topic::System,
            Payload::CycleStageChanged(stage_changed),
            "CycleManager",
        );
        let _ = bus.publish(msg).await;

        tokio::select! {
            _ = tokio::time::sleep(focus_duration) => {}
            _ = shutdown.wait_for_shutdown() => { break; }
        }

        if shutdown.is_shutdown() {
            break;
        }

        // ── REVIEW ──────────────────────────────────────────────────────
        state.current_stage = CycleStage::Review;
        tracing::info!(cycle = state.cycle_count, "Entering Review stage");

        let stage_changed = CycleStageChanged {
            cycle_id: state.cycle_id,
            new_stage: CycleStage::Review,
            focus_set: state.focus_set.clone(),
            timestamp: now_nanos(),
        };
        let msg = bus_msg(
            Topic::System,
            Payload::CycleStageChanged(stage_changed),
            "CycleManager",
        );
        let _ = bus.publish(msg).await;

        tokio::select! {
            _ = tokio::time::sleep(review_duration) => {}
            _ = shutdown.wait_for_shutdown() => { break; }
        }

        if shutdown.is_shutdown() {
            break;
        }

        // Publish cycle summary.
        let summary = CycleSummary {
            cycle_id: state.cycle_id,
            stage_completed: CycleStage::Review,
            focus_set: state.focus_set.clone(),
            signals_generated: findings.len() as u32,
            trades_executed: 0,
            pnl: None,
            notes: format!("Cycle {} complete", state.cycle_count),
            timestamp: now_nanos(),
        };
        let msg = bus_msg(Topic::System, Payload::CycleSummary(summary), "CycleManager");
        let _ = bus.publish(msg).await;

        publish_heartbeat(
            &bus,
            AgentRunStatus::Running,
            Some(format!("Cycle {} complete", state.cycle_count)),
        )
        .await;

        if !config.auto_advance {
            tracing::info!("auto_advance=false, stopping after cycle {}", state.cycle_count);
            break;
        }
    }

    publish_heartbeat(&bus, AgentRunStatus::Stopped, Some("Shutdown".to_string())).await;
    tracing::info!("CycleManager exiting");
}
