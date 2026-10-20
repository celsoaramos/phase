//! Own-turn mana sink (MagicFinder report, 2026-10-19).
//!
//! Silverquill Campus: the AI paid "{4}, {T}: Scry 1" in its own upkeep and
//! then passed its main phases with Kaalia of the Vast still in the command
//! zone. The scry is just as good in the opponent's end step, where the mana
//! is free; in the AI's own turn it eats the mana the turn's spells need.

use engine::game::scenario::{GameScenario, P0, P1};
use engine::types::actions::GameAction;
use engine::types::game_state::{GameState, WaitingFor};
use engine::types::identifiers::ObjectId;
use engine::types::mana::ManaColor;
use engine::types::phase::Phase;
use engine::types::player::PlayerId;
use phase_ai::choose_action;
use phase_ai::config::{create_config, AiDifficulty, Platform};
use rand::rngs::SmallRng;
use rand::SeedableRng;

const CAMPUS_TEXT: &str = "{T}: Add {W} or {B}.\n{4}, {T}: Scry 1.";

const DIFFICULTIES: [AiDifficulty; 4] = [
    AiDifficulty::Easy,
    AiDifficulty::Medium,
    AiDifficulty::Hard,
    AiDifficulty::VeryHard,
];

fn choose(state: &GameState, difficulty: AiDifficulty) -> GameAction {
    let config = create_config(difficulty, Platform::Native);
    let mut rng = SmallRng::seed_from_u64(42);
    choose_action(state, P0, &config, &mut rng).expect("AI should return an action")
}

fn activates(action: &GameAction, source: ObjectId) -> bool {
    matches!(action, GameAction::ActivateAbility { source_id, .. } if *source_id == source)
}

fn offered(state: &GameState, source: ObjectId) -> bool {
    let config = create_config(AiDifficulty::VeryHard, Platform::Native);
    phase_ai::score_candidates(state, P0, &config)
        .iter()
        .any(|(action, _)| activates(action, source))
}

/// AI (P0) with Silverquill Campus, four Plains and a library to scry, at
/// `phase` of `active`'s turn with P0 holding priority.
fn campus_board(phase: Phase, active: PlayerId) -> (GameState, ObjectId) {
    let mut scenario = GameScenario::new();
    scenario.at_phase(phase);
    let campus = scenario
        .add_land_from_oracle(P0, "Silverquill Campus", CAMPUS_TEXT)
        .id();
    for _ in 0..4 {
        scenario.add_basic_land(P0, ManaColor::White);
    }
    scenario.with_library_top(P0, &["Plains", "Plains", "Plains"]);
    scenario.with_library_top(P1, &["Plains", "Plains", "Plains"]);
    let mut runner = scenario.build();
    let state = runner.state_mut();
    state.active_player = active;
    state.priority_player = P0;
    state.waiting_for = WaitingFor::Priority { player: P0 };
    (runner.state().clone(), campus)
}

#[test]
fn campus_scry_is_offered_at_the_opponents_end_step() {
    let (state, campus) = campus_board(Phase::End, P1);
    assert!(
        offered(&state, campus),
        "fixture premise: the Campus scry must be a legal candidate at the opponent's end step"
    );
}

#[test]
fn does_not_spend_mana_on_the_campus_scry_in_its_own_turn() {
    for phase in [
        Phase::Upkeep,
        Phase::Draw,
        Phase::PreCombatMain,
        Phase::PostCombatMain,
        Phase::End,
    ] {
        let (state, campus) = campus_board(phase, P0);
        for difficulty in DIFFICULTIES {
            let action = choose(&state, difficulty);
            assert!(
                !activates(&action, campus),
                "{difficulty:?} paid the Campus scry in its own {phase:?}; that mana belongs to the turn's spells"
            );
        }
    }
}
