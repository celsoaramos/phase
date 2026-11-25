//! Harmful Aura with no opponent to enchant (`tactical_gate/mf_flags_1125b.rs`,
//! MagicFinder flag 7dee6cac): with no creature on the opponent's side, the robot
//! cast Aether Meltdown (-4/-0) on its own creature only for the enters energy.

use engine::game::scenario::{GameScenario, P0, P1};
use engine::types::actions::GameAction;
use engine::types::game_state::{GameState, WaitingFor};
use engine::types::identifiers::ObjectId;
use engine::types::mana::{ManaColor, ManaCost, ManaCostShard};
use engine::types::phase::Phase;
use engine::types::player::PlayerId;
use phase_ai::config::{create_config, AiDifficulty, Platform};
use phase_ai::score_candidates;

const AETHER_MELTDOWN: &str = "Enchant creature\nWhen Aether Meltdown enters, you get {E}{E} (two energy counters).\nEnchanted creature gets -4/-0.";

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

fn engine_offers(state: &GameState, card: ObjectId) -> bool {
    let actions: Vec<GameAction> = engine::ai_support::build_decision_context(state)
        .candidates
        .iter()
        .map(|c| c.action.clone())
        .collect();
    offers_cast(&actions, card)
}

/// The robot (P0) in its main phase with a creature, two Islands and Aether
/// Meltdown in hand. `opponent_creature` gives the opponent a creature.
fn board(opponent_creature: bool) -> (GameState, ObjectId) {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.add_creature(P0, "Robot Creature", 3, 3);
    scenario.add_basic_land(P0, ManaColor::Blue);
    scenario.add_basic_land(P0, ManaColor::Blue);
    if opponent_creature {
        scenario.add_creature(P1, "Opponent Creature", 4, 4);
    }
    let meltdown = scenario
        .add_spell_to_hand(P0, "Aether Meltdown", false)
        .as_enchantment()
        .with_subtypes(vec!["Aura"])
        .from_oracle_text_with_keywords(&["Enchant"], AETHER_MELTDOWN)
        .with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::Blue],
            generic: 1,
        })
        .id();
    let runner = scenario.build();
    let mut state = runner.state().clone();
    state.phase = Phase::PreCombatMain;
    state.active_player = P0;
    state.priority_player = P0;
    state.waiting_for = WaitingFor::Priority { player: P0 };
    (state, meltdown)
}

#[test]
fn robot_does_not_cast_a_harmful_aura_with_only_its_own_creatures_to_enchant() {
    let (state, meltdown) = board(false);
    assert!(
        engine_offers(&state, meltdown),
        "o motor oferece a conjuração"
    );
    assert!(
        !offers_cast(&scored(&state, P0), meltdown),
        "só há criatura do robô: a aura -4/-0 cairia nela"
    );
}

#[test]
fn robot_may_cast_a_harmful_aura_on_an_opponent_creature() {
    let (state, meltdown) = board(true);
    assert!(engine_offers(&state, meltdown));
    assert!(
        offers_cast(&scored(&state, P0), meltdown),
        "há criatura do oponente: o gate não recusa"
    );
}
