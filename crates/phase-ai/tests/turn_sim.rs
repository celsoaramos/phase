//! Turn that simulates (MagicFinder, 2026-10-27): a main-phase decision of the
//! AI's own turn plays its best lines out to the start of its next turn.
//!
//! The strength question is answered by the lab (`ai-ladder tsim=0|1`); these
//! fixtures pin that the layer runs, stays inside the issued domain, and is
//! deterministic in measurement mode.

use engine::game::scenario::{GameScenario, P0, P1};
use engine::types::actions::GameAction;
use engine::types::mana::ManaColor;
use engine::types::phase::Phase;
use phase_ai::choose_action;
use phase_ai::config::{create_config, AiDifficulty, Platform};
use rand::rngs::SmallRng;
use rand::SeedableRng;

fn choose(state: &engine::types::game_state::GameState, simulate: bool) -> GameAction {
    let mut config = create_config(AiDifficulty::VeryHard, Platform::Wasm);
    config.turn_simulation = simulate;
    config = config.into_measurement(7);
    let mut rng = SmallRng::seed_from_u64(7);
    choose_action(state, P0, &config, &mut rng).expect("AI should act")
}

fn main_phase_with_choices() -> engine::game::scenario::GameRunner {
    let mut s = GameScenario::new();
    s.at_phase(Phase::PreCombatMain);
    s.add_basic_land(P0, ManaColor::Red);
    s.add_basic_land(P0, ManaColor::Red);
    s.add_bolt_to_hand(P0);
    s.add_creature_to_hand(P0, "Bear", 2, 2);
    s.add_creature(P0, "Squire", 1, 2);
    s.add_creature(P1, "Brute", 3, 3);
    for player in [P0, P1] {
        s.with_library_top(player, &["Mountain", "Mountain", "Mountain", "Mountain"]);
    }
    s.build()
}

#[test]
fn main_phase_decision_runs_the_playouts_and_stays_legal() {
    let runner = main_phase_with_choices();
    let state = runner.state();
    let (before, ..) = phase_ai::turn_sim_stats();
    let action = choose(state, true);
    let (after, ..) = phase_ai::turn_sim_stats();
    assert!(after > before, "the main-phase decision went through turn_sim");
    let legal = engine::ai_support::legal_actions(state);
    assert!(
        legal.contains(&action),
        "turn_sim answered outside the legal actions: {action:?}"
    );
}

#[test]
fn turn_simulation_is_deterministic_in_measurement_mode() {
    let runner = main_phase_with_choices();
    let state = runner.state();
    assert_eq!(choose(state, true), choose(state, true));
}
