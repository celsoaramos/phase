//! Choco, Seeker of Paradise — two selections over the same looked-at cards.
//!
//! "Whenever one or more Birds you control attack, look at that many cards from
//! the top of your library. You may put one of them into your hand. Then put any
//! number of land cards from among them onto the battlefield tapped and the rest
//! into your graveyard."
//!
//! The second "from among them" clause used to OVERWRITE the first: the trigger
//! became a single Dig that only put lands onto the battlefield and milled the
//! rest, and the hand step vanished. CR 608.2c (written order) + CR 701.20e: the
//! hand choice comes first, then the land choice is made among the cards the
//! first one left behind, and only that remainder goes to the graveyard.

use engine::game::combat::AttackTarget;
use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::types::actions::GameAction;
use engine::types::card_type::CoreType;
use engine::types::game_state::WaitingFor;
use engine::types::identifiers::ObjectId;
use engine::types::phase::Phase;
use engine::types::zones::Zone;

const CHOCO_ORACLE: &str = "Vigilance\nWhenever one or more Birds you control attack, look at that many cards from the top of your library. You may put one of them into your hand. Then put any number of land cards from among them onto the battlefield tapped and the rest into your graveyard.\nLandfall — Whenever a land you control enters, Choco, Seeker of Paradise gets +1/+0 until end of turn.";

struct Table {
    runner: GameRunner,
    land_a: ObjectId,
    spell: ObjectId,
    land_b: ObjectId,
    below: ObjectId,
}

/// Library from the top: Land A, Spell, Land B, Below (the 4th card, never looked at).
/// Three Birds attack, so Choco looks at exactly three cards.
fn attack_with_three_birds() -> Table {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let below = scenario.add_card_to_library_top(P0, "Below");
    let land_b = scenario.add_card_to_library_top(P0, "Land B");
    let spell = scenario.add_card_to_library_top(P0, "Spell");
    let land_a = scenario.add_card_to_library_top(P0, "Land A");
    let choco = scenario
        .add_creature_from_oracle(P0, "Choco, Seeker of Paradise", 3, 5, CHOCO_ORACLE)
        .with_subtypes(vec!["Bird"])
        .id();
    let bird_a = scenario
        .add_creature(P0, "Bird A", 1, 1)
        .with_subtypes(vec!["Bird"])
        .id();
    let bird_b = scenario
        .add_creature(P0, "Bird B", 1, 1)
        .with_subtypes(vec!["Bird"])
        .id();

    let mut runner = scenario.build();
    for land in [land_a, land_b] {
        let obj = runner.state_mut().objects.get_mut(&land).unwrap();
        obj.card_types.core_types = vec![CoreType::Land];
        obj.base_card_types = obj.card_types.clone();
    }

    runner.advance_to_combat();
    runner
        .declare_attackers(&[
            (choco, AttackTarget::Player(P1)),
            (bird_a, AttackTarget::Player(P1)),
            (bird_b, AttackTarget::Player(P1)),
        ])
        .expect("declare the three Birds");
    Table {
        runner,
        land_a,
        spell,
        land_b,
        below,
    }
}

fn next_dig_choice(runner: &mut GameRunner) -> (Vec<ObjectId>, Vec<ObjectId>, Option<Zone>) {
    for _ in 0..40 {
        match runner.state().waiting_for.clone() {
            WaitingFor::DigChoice {
                cards,
                selectable_cards,
                kept_destination,
                ..
            } => return (cards, selectable_cards, kept_destination),
            WaitingFor::OrderTriggers { triggers, .. } => {
                runner
                    .act(GameAction::OrderTriggers {
                        order: (0..triggers.len()).collect(),
                    })
                    .expect("order triggers");
            }
            WaitingFor::Priority { .. } => {
                runner
                    .act(GameAction::PassPriority)
                    .expect("pass priority toward Choco's DigChoice");
            }
            other => panic!("unexpected state before Choco's DigChoice: {other:?}"),
        }
    }
    panic!("Choco's attack trigger never reached a DigChoice");
}

fn zone_of(runner: &GameRunner, id: ObjectId) -> Zone {
    runner.state().objects.get(&id).expect("object").zone
}

#[test]
fn choco_puts_one_card_in_hand_then_lands_tapped_and_the_rest_in_graveyard() {
    let Table {
        mut runner,
        land_a,
        spell,
        land_b,
        below,
    } = attack_with_three_birds();

    // Step 1: one of the three looked-at cards into the hand.
    let (cards, selectable, dest) = next_dig_choice(&mut runner);
    assert_eq!(dest, Some(Zone::Hand));
    assert_eq!(cards.len(), 3, "one card per attacking Bird");
    for id in [land_a, spell, land_b] {
        assert!(cards.contains(&id) && selectable.contains(&id));
    }
    assert!(!cards.contains(&below));
    runner
        .act(GameAction::SelectCards { cards: vec![spell] })
        .expect("put the nonland card into the hand");

    // Step 2: lands from among the cards that are left.
    let (cards, selectable, dest) = next_dig_choice(&mut runner);
    assert_eq!(dest, Some(Zone::Battlefield));
    assert_eq!(cards.len(), 2, "the card put into the hand is no longer among them");
    assert!(!cards.contains(&spell));
    assert!(selectable.contains(&land_a) && selectable.contains(&land_b));
    runner
        .act(GameAction::SelectCards {
            cards: vec![land_a, land_b],
        })
        .expect("put both lands onto the battlefield");
    runner.advance_until_stack_empty();

    assert_eq!(zone_of(&runner, spell), Zone::Hand);
    for land in [land_a, land_b] {
        assert_eq!(zone_of(&runner, land), Zone::Battlefield);
        assert!(runner.state().objects.get(&land).unwrap().tapped, "lands enter tapped");
    }
    assert_eq!(zone_of(&runner, below), Zone::Library, "only three cards were looked at");
}

#[test]
fn choco_land_kept_in_hand_and_unchosen_cards_go_to_graveyard() {
    let Table {
        mut runner,
        land_a,
        spell,
        land_b,
        below,
    } = attack_with_three_birds();

    next_dig_choice(&mut runner);
    runner
        .act(GameAction::SelectCards {
            cards: vec![land_a],
        })
        .expect("a land may go to the hand too");

    let (cards, selectable, _) = next_dig_choice(&mut runner);
    assert_eq!(cards.len(), 2);
    assert_eq!(selectable, vec![land_b], "only land cards are selectable");
    runner
        .act(GameAction::SelectCards { cards: vec![] })
        .expect("any number includes zero");
    runner.advance_until_stack_empty();

    assert_eq!(zone_of(&runner, land_a), Zone::Hand);
    assert_eq!(zone_of(&runner, spell), Zone::Graveyard);
    assert_eq!(zone_of(&runner, land_b), Zone::Graveyard);
    assert_eq!(zone_of(&runner, below), Zone::Library);
}

#[test]
fn choco_hand_step_is_optional() {
    let Table {
        mut runner,
        land_a,
        spell,
        land_b,
        below,
    } = attack_with_three_birds();

    next_dig_choice(&mut runner);
    runner
        .act(GameAction::SelectCards { cards: vec![] })
        .expect("\"you may put one of them\" accepts no card");

    let (cards, _, _) = next_dig_choice(&mut runner);
    assert_eq!(cards.len(), 3, "nothing left the looked-at pile");
    runner
        .act(GameAction::SelectCards {
            cards: vec![land_b],
        })
        .expect("put one land onto the battlefield");
    runner.advance_until_stack_empty();

    assert_eq!(zone_of(&runner, land_b), Zone::Battlefield);
    assert_eq!(zone_of(&runner, land_a), Zone::Graveyard);
    assert_eq!(zone_of(&runner, spell), Zone::Graveyard);
    assert_eq!(zone_of(&runner, below), Zone::Library);
}
