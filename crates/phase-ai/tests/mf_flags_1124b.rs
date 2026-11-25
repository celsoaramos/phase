//! Symmetric sweeper that costs the robot more than the opponent
//! (`tactical_gate/mf_flags_1124b.rs`, MagicFinder flags ee2f4ebf and e347639b):
//! with lethal on the board, the robot tapped five Goblins for Skirk Fire Marshal
//! and dealt 10 damage to each creature, killing its own army.

use engine::game::scenario::{GameScenario, P0, P1};
use engine::types::actions::GameAction;
use engine::types::game_state::{GameState, WaitingFor};
use engine::types::identifiers::ObjectId;
use engine::types::phase::Phase;
use engine::types::player::PlayerId;
use phase_ai::config::{create_config, AiDifficulty, Platform};
use phase_ai::score_candidates;

const MARSHAL: &str = "Tap five untapped Goblins you control: This creature deals 10 damage to each creature and each player.";

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

/// `own` Goblins of `own_size`/`own_size` for the robot, `opp` creatures of
/// `opp_size` for the opponent at `opp_life`.
fn board(
    own: usize,
    own_size: i32,
    opp: usize,
    opp_size: i32,
    opp_life: i32,
) -> (GameState, ObjectId) {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::BeginCombat);
    let marshal = {
        let mut b = scenario.add_creature_from_oracle(P0, "Skirk Fire Marshal", 2, 2, MARSHAL);
        b.with_subtypes(vec!["Goblin"]);
        b.id()
    };
    for i in 0..own {
        let mut b = scenario.add_creature(P0, &format!("Goblin {i}"), own_size, own_size);
        b.with_subtypes(vec!["Goblin"]);
    }
    for i in 0..opp {
        scenario.add_creature(P1, &format!("Opponent Creature {i}"), opp_size, opp_size);
    }
    let runner = scenario.build();
    let mut state = runner.state().clone();
    state.phase = Phase::BeginCombat;
    state.active_player = P0;
    state.priority_player = P0;
    state.waiting_for = WaitingFor::Priority { player: P0 };
    state.players[0].life = 26;
    state.players[1].life = opp_life;
    (state, marshal)
}

#[test]
fn robot_does_not_sweep_its_own_bigger_army() {
    let (state, marshal) = board(12, 2, 3, 3, 24);
    assert!(engine_offers(&state, marshal), "o motor oferece a ativação");
    assert!(
        !offers_activation(&scored(&state, P0), marshal),
        "a varredura mataria 26 de poder do robô contra 9 do oponente"
    );
}

#[test]
fn robot_may_sweep_when_the_opponent_loses_more() {
    let (state, marshal) = board(5, 1, 4, 6, 24);
    assert!(engine_offers(&state, marshal));
    assert!(
        offers_activation(&scored(&state, P0), marshal),
        "a varredura mata 24 de poder do oponente contra 7 do robô: o gate não recusa"
    );
}

#[test]
fn robot_may_sweep_when_the_damage_kills_the_opponent() {
    let (state, marshal) = board(12, 2, 3, 3, 9);
    assert!(engine_offers(&state, marshal));
    assert!(
        offers_activation(&scored(&state, P0), marshal),
        "10 de dano no oponente a 9 de vida vence o jogo"
    );
}
