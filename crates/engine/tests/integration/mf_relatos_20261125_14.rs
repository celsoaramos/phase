//! Relatos da mesa do MagicFinder, rodada de 25/11/2026 14h.
//!
//! Magmakin Artillerist: "Whenever you discard one or more cards, this creature
//! deals that much damage to each opponent." Descartar dezenas de cartas na
//! limpeza causava só 1 de dano: o gatilho em lote não tem filtro de carta, e
//! "that much" caía no valor de um único evento `Discarded` (1).

use engine::game::scenario::{GameRunner, GameScenario, P0};
use engine::types::actions::GameAction;
use engine::types::game_state::WaitingFor;

const MAGMAKIN_ARTILLERIST: &str = "Whenever you discard one or more cards, this creature deals that much damage to each opponent.\nCycling {1}{R} ({1}{R}, Discard this card: Draw a card.)\nWhen you cycle this card, it deals 1 damage to each opponent.";

fn cleanup_with_hand(hand_size: usize) -> (GameRunner, Vec<engine::types::identifiers::ObjectId>) {
    let mut scenario = GameScenario::new();
    scenario.at_phase(engine::types::phase::Phase::Cleanup);
    scenario.add_creature_from_oracle(P0, "Magmakin Artillerist", 1, 4, MAGMAKIN_ARTILLERIST);
    let cards = (0..hand_size)
        .map(|index| scenario.add_card_to_hand(P0, &format!("Hand Card {index}")))
        .collect::<Vec<_>>();
    for index in 0..4 {
        scenario.add_card_to_library_top(P0, &format!("Library Card {index}"));
    }
    let mut runner = scenario.build();
    {
        let state = runner.state_mut();
        state.active_player = P0;
        state.priority_player = P0;
        state.phase = engine::types::phase::Phase::Cleanup;
        state.waiting_for = WaitingFor::DiscardToHandSize {
            player: P0,
            count: hand_size - 7,
            cards: cards.clone(),
        };
    }
    (runner, cards)
}

fn resolve_stack(runner: &mut GameRunner) {
    for _ in 0..8 {
        if runner.state().stack.is_empty() {
            return;
        }
        runner
            .act(GameAction::PassPriority)
            .expect("pass priority to resolve Magmakin");
    }
    assert!(
        runner.state().stack.is_empty(),
        "Magmakin trigger must resolve"
    );
}

#[test]
fn magmakin_artillerist_deals_damage_equal_to_cards_discarded_in_cleanup() {
    let (mut runner, cards) = cleanup_with_hand(12);
    runner
        .act(GameAction::SelectCards {
            cards: cards[..5].to_vec(),
        })
        .expect("discard five cards to hand size");
    resolve_stack(&mut runner);
    assert_eq!(
        runner.state().players[1].life,
        15,
        "five cards discarded at once = 5 damage to each opponent"
    );
}

#[test]
fn magmakin_artillerist_single_discard_still_deals_one() {
    let (mut runner, cards) = cleanup_with_hand(8);
    runner
        .act(GameAction::SelectCards {
            cards: vec![cards[0]],
        })
        .expect("discard one card to hand size");
    resolve_stack(&mut runner);
    assert_eq!(runner.state().players[1].life, 19);
}
