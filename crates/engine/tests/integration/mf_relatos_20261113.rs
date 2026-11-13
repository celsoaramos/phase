//! Relatos da mesa (13/11/2026, rodada das 21h de 12/11).
//!
//! Tinybones, Bauble Burglar — "Whenever an opponent discards a card, exile it
//! from their graveyard with a stash counter on it. During your turn, you may
//! play cards you don't own with stash counters on them from exile, and mana of
//! any type can be spent to cast those spells." The play permission was an
//! unparsed line, so the stashed cards were never offered (relato: "não consigo
//! usar a habilidade dele de jogar cartas exiladas com o marcador").

use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::types::actions::GameAction;
use engine::types::counter::CounterType;
use engine::types::game_state::{CastPaymentMode, WaitingFor};
use engine::types::identifiers::ObjectId;
use engine::types::phase::Phase;
use engine::types::zones::Zone;

const TINYBONES: &str = "Whenever an opponent discards a card, exile it from their graveyard with a stash counter on it.\nDuring your turn, you may play cards you don't own with stash counters on them from exile, and mana of any type can be spent to cast those spells.\n{3}{B}, {T}: Each opponent discards a card. Activate only as a sorcery.";

fn stash_count(runner: &GameRunner, id: ObjectId) -> u32 {
    runner.state().objects[&id]
        .counters
        .get(&CounterType::Generic("stash".to_string()))
        .copied()
        .unwrap_or(0)
}

fn drain_stack(runner: &mut GameRunner) {
    for _ in 0..40 {
        match runner.state().waiting_for.clone() {
            WaitingFor::Priority { .. } if !runner.state().stack.is_empty() => {
                if runner.act(GameAction::PassPriority).is_err() {
                    break;
                }
            }
            _ => break,
        }
    }
}

/// P0 controls Tinybones and makes P1 discard their only card.
fn stash_opponent_card(opponent_card_is_land: bool) -> (GameRunner, ObjectId) {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.add_creature_from_oracle(P0, "Tinybones, Bauble Burglar", 1, 3, TINYBONES);
    let discard = scenario
        .add_spell_to_hand_from_oracle(P0, "Mind Squeeze", false, "Each opponent discards a card.")
        .id();
    let stolen = if opponent_card_is_land {
        scenario.add_land_to_hand(P1, "Opponent Land").id()
    } else {
        scenario.add_creature_to_hand(P1, "Opponent Bear", 2, 2).id()
    };
    let mut runner = scenario.build();
    runner
        .act(GameAction::CastSpell {
            object_id: discard,
            card_id: runner.state().objects[&discard].card_id,
            targets: vec![],
            payment_mode: CastPaymentMode::Auto,
        })
        .expect("discard spell cast");
    drain_stack(&mut runner);
    (runner, stolen)
}

#[test]
fn tinybones_stashes_the_discarded_card() {
    let (runner, stolen) = stash_opponent_card(false);
    assert_eq!(runner.state().objects[&stolen].zone, Zone::Exile);
    assert_eq!(stash_count(&runner, stolen), 1);
}

#[test]
fn tinybones_offers_the_stashed_spell_and_casts_it() {
    let (mut runner, stolen) = stash_opponent_card(false);
    let legal = engine::ai_support::legal_actions(runner.state());
    assert!(
        legal.iter().any(|a| matches!(
            a,
            GameAction::CastSpell { object_id, .. } if *object_id == stolen
        )),
        "the stashed card is castable during Tinybones' controller's turn: {legal:?}"
    );
    runner
        .act(GameAction::CastSpell {
            object_id: stolen,
            card_id: runner.state().objects[&stolen].card_id,
            targets: vec![],
            payment_mode: CastPaymentMode::Auto,
        })
        .expect("stashed card cast from exile");
    drain_stack(&mut runner);
    let obj = &runner.state().objects[&stolen];
    assert_eq!(obj.zone, Zone::Battlefield);
    assert_eq!(obj.controller, P0);
}

#[test]
fn tinybones_offers_the_stashed_land() {
    let (runner, stolen) = stash_opponent_card(true);
    let legal = engine::ai_support::legal_actions(runner.state());
    assert!(
        legal.iter().any(|a| matches!(
            a,
            GameAction::PlayLand { object_id, .. } if *object_id == stolen
        )),
        "a stashed land can be played (\"play cards\"): {legal:?}"
    );
}

/// "During your turn": nothing is offered on the opponent's turn.
#[test]
fn tinybones_permission_is_only_during_your_turn() {
    let (mut runner, stolen) = stash_opponent_card(false);
    runner.state_mut().active_player = P1;
    let (actions, _, _) = engine::ai_support::legal_actions_for_viewer(runner.state(), P0);
    assert!(!actions.iter().any(|a| matches!(
        a,
        GameAction::CastSpell { object_id, .. } if *object_id == stolen
    )));
}

/// "Cards you don't own": your own card with a stash counter is not offered.
#[test]
fn tinybones_does_not_offer_your_own_stashed_card() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.add_creature_from_oracle(P0, "Tinybones, Bauble Burglar", 1, 3, TINYBONES);
    let own = scenario.add_creature_to_exile(P0, "Own Bear", 2, 2).id();
    let mut runner = scenario.build();
    runner
        .state_mut()
        .objects
        .get_mut(&own)
        .unwrap()
        .counters
        .insert(CounterType::Generic("stash".to_string()), 1);
    let legal = engine::ai_support::legal_actions(runner.state());
    assert!(!legal.iter().any(|a| matches!(
        a,
        GameAction::CastSpell { object_id, .. } if *object_id == own
    )));
}
