//! Relatos da mesa do MagicFinder, rodada de 26/11/2026 19h.
//!
//! - Lord Windgrace: "+2: Discard a card, then draw a card. If a land card is
//!   discarded this way, draw an additional card." comprava UMA carta depois de
//!   descartar uma Montanha. O "discarded this way" virava `ZoneChangedThisWay`,
//!   que lê o que a instrução ANTERIOR moveu — a compra, não o descarte.

use engine::game::scenario::{GameRunner, GameScenario, P0};
use engine::types::actions::GameAction;
use engine::types::game_state::WaitingFor;
use engine::types::identifiers::ObjectId;
use engine::types::phase::Phase;

const LORD_WINDGRACE: &str = "+2: Discard a card, then draw a card. If a land card is discarded this way, draw an additional card.";

fn walker(oracle: &str, name: &str, discard_land: bool) -> (GameRunner, ObjectId, ObjectId) {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let walker = scenario
        .add_planeswalker_from_oracle(P0, name, "Test", 5, oracle)
        .id();
    let discarded = if discard_land {
        scenario.add_land_to_hand(P0, "Mountain").id()
    } else {
        scenario
            .add_creature_to_hand(P0, "Grizzly Bears", 2, 2)
            .id()
    };
    for _ in 0..4 {
        scenario.add_card_to_library_top(P0, "Library Card");
    }
    (scenario.build(), walker, discarded)
}

/// Activate the walker's only ability and resolve it, discarding `card`.
fn activate_discarding(runner: &mut GameRunner, walker: ObjectId, card: ObjectId) {
    runner
        .act(GameAction::ActivateAbility {
            source_id: walker,
            ability_index: 0,
            payment_mode: Default::default(),
        })
        .expect("the loyalty ability is activatable");
    for _ in 0..20 {
        match runner.state().waiting_for.clone() {
            WaitingFor::DiscardChoice { .. } => {
                runner
                    .act(GameAction::SelectCards { cards: vec![card] })
                    .expect("discard the chosen card");
            }
            WaitingFor::Priority { .. } if runner.state().stack.is_empty() => break,
            WaitingFor::Priority { .. } => {
                runner
                    .act(GameAction::PassPriority)
                    .expect("resolve the ability");
            }
            other => panic!("unexpected prompt {other:?}"),
        }
    }
    assert!(runner.state().stack.is_empty(), "the ability resolved");
    assert!(
        runner.state().players[0].graveyard.contains(&card),
        "the chosen card was discarded"
    );
}

#[test]
fn lord_windgrace_draws_two_after_discarding_a_land() {
    let (mut runner, walker, mountain) = walker(LORD_WINDGRACE, "Lord Windgrace", true);
    activate_discarding(&mut runner, walker, mountain);
    assert_eq!(
        runner.state().players[0].hand.len(),
        2,
        "a land card was discarded this way: draw a card and an additional card"
    );
}

#[test]
fn lord_windgrace_draws_one_after_discarding_a_nonland() {
    let (mut runner, walker, bears) = walker(LORD_WINDGRACE, "Lord Windgrace", false);
    activate_discarding(&mut runner, walker, bears);
    assert_eq!(
        runner.state().players[0].hand.len(),
        1,
        "a nonland card was discarded: only the first draw"
    );
}
