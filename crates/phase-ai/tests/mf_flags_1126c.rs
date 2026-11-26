//! MagicFinder flag of 26/11/2026 (`tactical_gate/mf_flags_1126c.rs`).
//!
//! - 6eec8f76: the robot (seat 1) activated Station on Class-4 Tactical Cube with
//!   88 charge counters, every threshold being 7+. U.S.S. Prometheus, Prototype
//!   (13 counters, thresholds 4+ and 8+) was offered the same way.

use engine::types::actions::GameAction;
use engine::types::counter::CounterType;
use engine::types::game_state::GameState;
use engine::types::identifiers::ObjectId;
use engine::types::player::PlayerId;
use phase_ai::config::{create_config, AiDifficulty, Platform};
use phase_ai::saved_state::load_saved_game_state;
use phase_ai::score_candidates;
use std::io::Read;

const P1: PlayerId = PlayerId(1);

fn position() -> GameState {
    let mut raw = String::new();
    flate2::read::GzDecoder::new(
        &include_bytes!("fixtures/scenarios/mf-flag-6eec8f76-station.json.gz")[..],
    )
    .read_to_string(&mut raw)
    .expect("fixture inflates");
    load_saved_game_state(&raw).expect("position restores")
}

fn object_named(state: &GameState, name: &str) -> ObjectId {
    state
        .battlefield
        .iter()
        .copied()
        .find(|id| state.objects[id].name == name)
        .unwrap_or_else(|| panic!("{name} on the battlefield"))
}

fn stations(actions: &[GameAction]) -> Vec<ObjectId> {
    actions
        .iter()
        .filter_map(|a| match a {
            GameAction::ActivateStation { spacecraft_id, .. } => Some(*spacecraft_id),
            _ => None,
        })
        .collect()
}

fn engine_stations(state: &GameState) -> Vec<ObjectId> {
    let actions: Vec<GameAction> = engine::ai_support::build_decision_context(state)
        .candidates
        .iter()
        .map(|c| c.action.clone())
        .collect();
    stations(&actions)
}

fn scored_stations(state: &GameState) -> Vec<ObjectId> {
    let config = create_config(AiDifficulty::VeryHard, Platform::Native);
    let actions: Vec<GameAction> = score_candidates(state, P1, &config)
        .into_iter()
        .map(|(action, _)| action)
        .collect();
    stations(&actions)
}

#[test]
fn flag_6eec8f76_station_past_every_threshold_is_not_activated() {
    let state = position();
    let cube = object_named(&state, "Class-4 Tactical Cube");
    let prometheus = object_named(&state, "U.S.S. Prometheus, Prototype");
    let offered = engine_stations(&state);
    assert!(offered.contains(&cube), "o motor oferece o Station do Cube");
    assert!(
        offered.contains(&prometheus),
        "o motor oferece o Station da Prometheus"
    );
    let scored = scored_stations(&state);
    assert!(
        !scored.contains(&cube),
        "88 marcadores e todo limiar em 7+: mais marcadores não mudam nada"
    );
    assert!(
        !scored.contains(&prometheus),
        "13 marcadores e limiares 4+ e 8+: mais marcadores não mudam nada"
    );
}

#[test]
fn station_below_its_last_threshold_stays_open() {
    let mut state = position();
    let prometheus = object_named(&state, "U.S.S. Prometheus, Prototype");
    state
        .objects
        .get_mut(&prometheus)
        .unwrap()
        .counters
        .insert(CounterType::Generic("charge".to_string()), 5);
    assert!(engine_stations(&state).contains(&prometheus));
    assert!(
        scored_stations(&state).contains(&prometheus),
        "com 5 marcadores o limiar 8+ ainda não foi atingido"
    );
}
