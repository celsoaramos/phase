//! Untap with nothing of the robot's to untap (`tactical_gate/mf_flags_1125.rs`,
//! MagicFinder flags e8ee3d01 and 3d498e2e): in its own beginning of combat, with
//! every creature untapped, the robot returned an Elf to hand to activate Wirewood
//! Symbiote.

use engine::game::scenario::{GameScenario, P0, P1};
use engine::types::actions::GameAction;
use engine::types::game_state::{GameState, WaitingFor};
use engine::types::identifiers::ObjectId;
use engine::types::phase::Phase;
use engine::types::player::PlayerId;
use phase_ai::config::{create_config, AiDifficulty, Platform};
use phase_ai::score_candidates;

const SYMBIOTE: &str = "Return an Elf you control to its owner's hand: Untap target creature. Activate only once each turn.";

fn scored(state: &GameState, player: PlayerId) -> Vec<GameAction> {
    let config = create_config(AiDifficulty::VeryHard, Platform::Native);
    score_candidates(state, player, &config)
        .into_iter()
        .map(|(action, _)| action)
        .collect()
}

fn offers_activation(actions: &[GameAction], source: ObjectId) -> bool {
    actions
        .iter()
        .any(|a| matches!(a, GameAction::ActivateAbility { source_id, .. } if *source_id == source))
}

fn engine_offers(state: &GameState, source: ObjectId) -> bool {
    let actions: Vec<GameAction> = engine::ai_support::build_decision_context(state)
        .candidates
        .iter()
        .map(|c| c.action.clone())
        .collect();
    offers_activation(&actions, source)
}

/// The robot (P0) with Wirewood Symbiote and another Elf; the opponent with
/// tapped creatures. `own_tapped` taps the robot's other Elf; `rebuy` gives that
/// Elf an enters trigger.
fn board(own_tapped: bool, rebuy: bool) -> (GameState, ObjectId) {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::BeginCombat);
    let symbiote = {
        let mut b = scenario.add_creature_from_oracle(P0, "Wirewood Symbiote", 1, 1, SYMBIOTE);
        b.with_subtypes(vec!["Insect"]);
        b.id()
    };
    let elf = {
        let mut b = if rebuy {
            scenario.add_creature_from_oracle(
                P0,
                "Elvish Visionary",
                1,
                1,
                "When this creature enters, draw a card.",
            )
        } else {
            scenario.add_creature(P0, "Heritage Druid", 1, 1)
        };
        b.with_subtypes(vec!["Elf"]);
        b.id()
    };
    let foes: Vec<ObjectId> = (0..3)
        .map(|i| {
            scenario
                .add_creature(P1, &format!("Opponent Creature {i}"), 2, 2)
                .id()
        })
        .collect();
    let runner = scenario.build();
    let mut state = runner.state().clone();
    for foe in foes {
        state.objects.get_mut(&foe).unwrap().tapped = true;
    }
    state.objects.get_mut(&elf).unwrap().tapped = own_tapped;
    state.phase = Phase::BeginCombat;
    state.active_player = P0;
    state.priority_player = P0;
    state.waiting_for = WaitingFor::Priority { player: P0 };
    (state, symbiote)
}

#[test]
fn robot_does_not_untap_when_nothing_of_its_own_is_tapped() {
    let (state, symbiote) = board(false, false);
    assert!(
        engine_offers(&state, symbiote),
        "o motor oferece a ativação"
    );
    assert!(
        !offers_activation(&scored(&state, P0), symbiote),
        "só há criaturas viradas do oponente: desvirar não serve ao robô"
    );
}

#[test]
fn robot_may_untap_its_own_tapped_creature() {
    let (state, symbiote) = board(true, false);
    assert!(engine_offers(&state, symbiote));
    assert!(
        offers_activation(&scored(&state, P0), symbiote),
        "o Elfo do robô está virado: o gate não recusa"
    );
}

#[test]
fn robot_may_rebuy_an_enters_trigger() {
    let (state, symbiote) = board(false, true);
    assert!(engine_offers(&state, symbiote));
    assert!(
        offers_activation(&scored(&state, P0), symbiote),
        "devolver o Elvish Visionary recompra o 'quando entra': o gate não recusa"
    );
}
