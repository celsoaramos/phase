//! MagicFinder flag of 26/11/2026 (`tactical_gate/mf_flags_1126b.rs`, 27/11 rule).
//!
//! - fd448b10: Heroic Intervention cast in response to an opponent's Meteorite
//!   trigger ("2 damage to any target") aimed at the robot itself. The robot
//!   controlled only two lands; the protection reaches permanents only.

use engine::types::actions::GameAction;
use engine::types::game_state::GameState;
use engine::types::identifiers::ObjectId;
use engine::types::player::PlayerId;
use phase_ai::config::{create_config, AiDifficulty, Platform};
use phase_ai::saved_state::load_saved_game_state;
use phase_ai::score_candidates;
use std::io::Read;

fn scored(state: &GameState, player: PlayerId) -> Vec<GameAction> {
    let config = create_config(AiDifficulty::VeryHard, Platform::Native);
    score_candidates(state, player, &config)
        .into_iter()
        .map(|(action, _)| action)
        .collect()
}

fn offers_cast(actions: &[GameAction], card: ObjectId) -> bool {
    actions
        .iter()
        .any(|a| matches!(a, GameAction::CastSpell { object_id, .. } if *object_id == card))
}

#[test]
fn flag_fd448b10_protection_against_damage_aimed_at_the_player_is_not_cast() {
    let mut raw = String::new();
    flate2::read::GzDecoder::new(
        &include_bytes!("fixtures/scenarios/mf-flag-fd448b10-heroic-intervention.json.gz")[..],
    )
    .read_to_string(&mut raw)
    .expect("fixture inflates");
    let state = load_saved_game_state(&raw).expect("position restores");
    let robot = PlayerId(1);
    let spell = state
        .players
        .iter()
        .find(|p| p.id == robot)
        .expect("robot")
        .hand
        .iter()
        .copied()
        .find(|id| state.objects[id].name == "Heroic Intervention")
        .expect("Heroic Intervention in the robot's hand");
    let engine_actions: Vec<GameAction> = engine::ai_support::build_decision_context(&state)
        .candidates
        .iter()
        .map(|c| c.action.clone())
        .collect();
    assert!(offers_cast(&engine_actions, spell), "o motor oferece a mágica");
    assert!(
        !offers_cast(&scored(&state, robot), spell),
        "o gatilho do oponente mira o jogador: a proteção não salva nada"
    );
}
