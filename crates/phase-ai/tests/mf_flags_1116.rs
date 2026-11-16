//! Sacrifice-for-a-counter outside any window (`tactical_gate/mf_flags_1116.rs`,
//! MagicFinder flag f4f895c5): the robot activated Arcbound Ravager in its own draw
//! step, sacrificing the 6/6 Ravager itself to move the counters onto a 2/2
//! Construct. The engine offers the activation and the AI scores it; the gate is what
//! removes it when nothing can make the sacrifice worth an artifact.

use engine::game::combat::{AttackerInfo, CombatState};
use engine::game::scenario::{GameScenario, P0, P1};
use engine::types::actions::GameAction;
use engine::types::game_state::{GameState, WaitingFor};
use engine::types::identifiers::ObjectId;
use engine::types::phase::Phase;
use engine::types::player::PlayerId;
use phase_ai::config::{create_config, AiDifficulty, Platform};
use phase_ai::score_candidates;

const RAVAGER: &str = "Sacrifice an artifact: Put a +1/+1 counter on this creature.\nModular 1";

fn scored(state: &GameState, player: PlayerId) -> Vec<GameAction> {
    let config = create_config(AiDifficulty::VeryHard, Platform::Native);
    score_candidates(state, player, &config)
        .into_iter()
        .map(|(action, _)| action)
        .collect()
}

fn offers_activation(actions: &[GameAction], source: ObjectId) -> bool {
    actions.iter().any(
        |a| matches!(a, GameAction::ActivateAbility { source_id, .. } if *source_id == source),
    )
}

fn engine_offers_activation(state: &GameState, source: ObjectId) -> bool {
    let actions: Vec<GameAction> = engine::ai_support::build_decision_context(state)
        .candidates
        .iter()
        .map(|c| c.action.clone())
        .collect();
    offers_activation(&actions, source)
}

fn set_priority(state: &mut GameState, phase: Phase) {
    state.phase = phase;
    state.active_player = P0;
    state.priority_player = P0;
    state.waiting_for = WaitingFor::Priority { player: P0 };
}

struct Board {
    state: GameState,
    ravager: ObjectId,
}

/// The flagged board: Arcbound Ravager with six +1/+1 counters and a Construct token
/// for the robot; three small creatures for the opponent.
fn board(phase: Phase, fodder_text: Option<&str>) -> Board {
    let mut scenario = GameScenario::new();
    scenario.at_phase(phase);
    let ravager = {
        let mut b = scenario.add_creature_from_oracle(P0, "Arcbound Ravager", 0, 0, RAVAGER);
        b.as_artifact_creature().with_plus_counters(6);
        b.id()
    };
    {
        let mut b = match fodder_text {
            Some(text) => scenario.add_creature_from_oracle(P0, "Construct", 2, 2, text),
            None => scenario.add_creature(P0, "Construct", 2, 2),
        };
        b.as_artifact_creature();
    }
    scenario.add_creature(P1, "Kinscaer Sentry", 2, 2);
    scenario.add_creature(P1, "Kinscaer Sentry", 2, 2);
    scenario.add_creature(P1, "Kithkin Soldier", 1, 1);
    let runner = scenario.build();
    let mut state = runner.state().clone();
    set_priority(&mut state, phase);
    Board { state, ravager }
}

fn put_in_combat(state: &mut GameState, attacker: ObjectId) {
    state.phase = Phase::DeclareBlockers;
    state.combat = Some(CombatState {
        attackers: vec![AttackerInfo::attacking_player(attacker, P1)],
        ..CombatState::default()
    });
}

#[test]
fn sacrifice_for_a_counter_in_the_draw_step_is_not_activated() {
    let b = board(Phase::Draw, None);
    assert!(
        engine_offers_activation(&b.state, b.ravager),
        "precondition: the engine offers the Ravager activation in the draw step"
    );
    assert!(
        !offers_activation(&scored(&b.state, P0), b.ravager),
        "no stack, no combat, no death payoff: the sacrifice only throws a card away"
    );
}

#[test]
fn sacrifice_for_a_counter_before_combat_is_not_activated() {
    let b = board(Phase::PreCombatMain, None);
    assert!(engine_offers_activation(&b.state, b.ravager));
    assert!(
        !offers_activation(&scored(&b.state, P0), b.ravager),
        "the counters do the same work after blockers are declared"
    );
}

#[test]
fn fodder_with_a_death_payoff_keeps_the_activation() {
    let b = board(
        Phase::PreCombatMain,
        Some("When this creature dies, draw a card."),
    );
    assert!(engine_offers_activation(&b.state, b.ravager));
    assert!(
        offers_activation(&scored(&b.state, P0), b.ravager),
        "a death payoff on the fodder is value: the gate must stay out of the way"
    );
}

#[test]
fn sacrifice_in_combat_stays_available() {
    let mut b = board(Phase::DeclareBlockers, None);
    put_in_combat(&mut b.state, b.ravager);
    assert!(
        engine_offers_activation(&b.state, b.ravager),
        "precondition: the engine offers the activation in combat"
    );
    assert!(
        offers_activation(&scored(&b.state, P0), b.ravager),
        "in combat the counters change damage: the gate must stay out of the way"
    );
}
