//! Combat trick before combat (`tactical_gate/mf_flags_1124.rs`, MagicFinder flag
//! 76e88b36): in its own first main phase the robot cast Supernatural Stamina on its
//! Voldaren Bloodcaster, showing the trick and spending the death protection with an
//! empty stack. The engine offers the cast and the AI scores it; the gate removes it
//! while the combat window is still ahead.

use engine::game::combat::{AttackerInfo, CombatState};
use engine::game::scenario::{GameScenario, P0, P1};
use engine::types::actions::GameAction;
use engine::types::game_state::{GameState, WaitingFor};
use engine::types::identifiers::ObjectId;
use engine::types::phase::Phase;
use engine::types::player::PlayerId;
use phase_ai::config::{create_config, AiDifficulty, Platform};
use phase_ai::score_candidates;

const STAMINA: &str = "Until end of turn, target creature gets +2/+0 and gains \"When this creature dies, return it to the battlefield tapped under its owner's control.\"";

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

fn engine_offers_cast(state: &GameState, card: ObjectId) -> bool {
    let actions: Vec<GameAction> = engine::ai_support::build_decision_context(state)
        .candidates
        .iter()
        .map(|c| c.action.clone())
        .collect();
    offers_cast(&actions, card)
}

struct Board {
    state: GameState,
    trick: ObjectId,
    bloodcaster: ObjectId,
}

/// The flagged board: Voldaren Bloodcaster for the robot, Supernatural Stamina in
/// hand; Josu Vess, Wilhelt and a Zombie for the opponent.
fn board(phase: Phase, trick: (&str, &str), prowess: bool) -> Board {
    let mut scenario = GameScenario::new();
    scenario.at_phase(phase);
    let bloodcaster = {
        let mut b = scenario.add_creature(P0, "Voldaren Bloodcaster", 2, 1);
        b.flying();
        b.id()
    };
    if prowess {
        scenario.add_creature_from_oracle(P0, "Monastery Swiftspear", 1, 2, "Haste\nProwess");
    }
    let trick = scenario
        .add_spell_to_hand_from_oracle(P0, trick.0, true, trick.1)
        .id();
    scenario.add_creature(P1, "Josu Vess, Lich Knight", 4, 5);
    scenario.add_creature(P1, "Wilhelt, the Rotcleaver", 3, 3);
    scenario.add_creature(P1, "Zombie", 2, 2);
    let runner = scenario.build();
    let mut state = runner.state().clone();
    state.phase = phase;
    state.active_player = P0;
    state.priority_player = P0;
    state.waiting_for = WaitingFor::Priority { player: P0 };
    Board {
        state,
        trick,
        bloodcaster,
    }
}

#[test]
fn stamina_before_combat_is_not_cast() {
    let b = board(
        Phase::PreCombatMain,
        ("Supernatural Stamina", STAMINA),
        false,
    );
    assert!(
        engine_offers_cast(&b.state, b.trick),
        "precondition: the engine offers Supernatural Stamina in the first main phase"
    );
    assert!(
        !offers_cast(&scored(&b.state, P0), b.trick),
        "empty stack before combat: the trick does the same work after blockers"
    );
}

#[test]
fn giant_growth_before_combat_is_not_cast() {
    let b = board(
        Phase::PreCombatMain,
        (
            "Giant Growth",
            "Target creature gets +3/+3 until end of turn.",
        ),
        false,
    );
    assert!(engine_offers_cast(&b.state, b.trick));
    assert!(!offers_cast(&scored(&b.state, P0), b.trick));
}

#[test]
fn stamina_in_combat_stays_available() {
    let mut b = board(
        Phase::DeclareBlockers,
        ("Supernatural Stamina", STAMINA),
        false,
    );
    b.state.combat = Some(CombatState {
        attackers: vec![AttackerInfo::attacking_player(b.bloodcaster, P1)],
        ..CombatState::default()
    });
    assert!(engine_offers_cast(&b.state, b.trick));
    assert!(
        offers_cast(&scored(&b.state, P0), b.trick),
        "after blockers the gate must stay out of the way"
    );
}

#[test]
fn a_cast_trigger_keeps_the_precombat_cast() {
    let b = board(
        Phase::PreCombatMain,
        ("Supernatural Stamina", STAMINA),
        true,
    );
    assert!(engine_offers_cast(&b.state, b.trick));
    assert!(
        offers_cast(&scored(&b.state, P0), b.trick),
        "prowess pays for the cast itself: fail open"
    );
}

#[test]
fn evasion_grant_before_combat_is_not_gated() {
    let b = board(
        Phase::PreCombatMain,
        (
            "Distortion Strike",
            "Target creature gets +1/+0 until end of turn and can't be blocked this turn.",
        ),
        false,
    );
    assert!(engine_offers_cast(&b.state, b.trick));
    assert!(
        offers_cast(&scored(&b.state, P0), b.trick),
        "unblockable matters before blocks: not this gate's call"
    );
}
