//! The "discard X cards" additional-cost class beyond the two headline cards
//! (Restless Dreams, Firestorm):
//!
//! - X = 0 on an additional cost: nothing is discarded, no target is chosen
//!   (CR 107.3a — 0 is a legal announcement; CR 601.2c).
//! - A TYPED count (Scorched Earth, "discard X land cards"): the announced
//!   maximum counts only cards that can pay (CR 601.2b), and only they pay.

use engine::game::scenario::{GameScenario, P0, P1};
use engine::types::ability::TargetRef;
use engine::types::actions::GameAction;
use engine::types::game_state::WaitingFor;
use engine::types::identifiers::ObjectId;
use engine::types::mana::{ManaColor, ManaCost, ManaCostShard, ManaType, ManaUnit};
use engine::types::phase::Phase;
use engine::types::zones::Zone;

fn add_mana(runner: &mut engine::game::scenario::GameRunner, ty: ManaType, count: usize) {
    for _ in 0..count {
        let unit = ManaUnit::new(ty, ObjectId(0), false, vec![]);
        runner.state_mut().players[0].mana_pool.add(unit);
    }
}

#[test]
fn restless_dreams_x_zero_discards_nothing_and_returns_nothing() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let spell = {
        let mut b = scenario.add_spell_to_hand_from_oracle(
            P0,
            "Restless Dreams",
            false,
            "As an additional cost to cast this spell, discard X cards.\n\
             Return X target creature cards from your graveyard to your hand.",
        );
        b.with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::Black],
            generic: 0,
        });
        b.id()
    };
    let dead = scenario
        .add_creature_to_graveyard(P0, "Dead Bear", 2, 2)
        .id();
    let spare = scenario.add_card_to_hand(P0, "Spare Card");
    let mut runner = scenario.build();
    add_mana(&mut runner, ManaType::Black, 1);

    let outcome = runner.cast(spell).x(0).resolve();

    outcome.assert_zone(&[spare], Zone::Hand);
    outcome.assert_zone(&[dead], Zone::Graveyard);
    outcome.assert_zone(&[spell], Zone::Graveyard);
}

#[test]
fn scorched_earth_typed_discard_x_counts_and_pays_with_land_cards_only() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let spell = {
        let mut b = scenario.add_spell_to_hand_from_oracle(
            P0,
            "Scorched Earth",
            false,
            "As an additional cost to cast this spell, discard X land cards.\n\
             Destroy X target lands.",
        );
        b.with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::Red],
            generic: 3,
        });
        b.id()
    };
    let hand_lands: Vec<ObjectId> = (0..2)
        .map(|i| {
            scenario
                .add_land_to_hand(P0, &format!("Spare Land {i}"))
                .id()
        })
        .collect();
    let nonland = scenario.add_card_to_hand(P0, "Spare Spell");
    let their_lands: Vec<ObjectId> = (0..3)
        .map(|_| scenario.add_basic_land(P1, ManaColor::Green))
        .collect();
    let mut runner = scenario.build();
    add_mana(&mut runner, ManaType::Red, 4);

    // CR 601.2b: the announced maximum is the number of LAND cards in hand (2),
    // not the hand size (3).
    let card_id = runner.state().objects[&spell].card_id;
    let waiting = runner
        .act(GameAction::CastSpell {
            object_id: spell,
            card_id,
            targets: Vec::new(),
            payment_mode: Default::default(),
        })
        .expect("Scorched Earth must be castable")
        .waiting_for;
    match waiting {
        WaitingFor::ChooseXValue { min, max, .. } => {
            assert_eq!((min, max), (0, 2), "X is capped by land cards in hand");
        }
        other => panic!("expected ChooseXValue, got {other:?}"),
    }
    runner
        .act(GameAction::ChooseX { value: 2 })
        .expect("X = 2 must be accepted");
    runner
        .act(GameAction::SelectTargets {
            targets: vec![
                TargetRef::Object(their_lands[0]),
                TargetRef::Object(their_lands[1]),
            ],
        })
        .expect("two lands must be accepted as targets");
    // Only the two land cards may pay; a nonland card is not a choice.
    match &runner.state().waiting_for {
        WaitingFor::PayCost { choices, count, .. } => {
            assert_eq!(*count, 2);
            assert!(choices.contains(&hand_lands[0]) && choices.contains(&hand_lands[1]));
            assert!(!choices.contains(&nonland), "a nonland card cannot pay");
        }
        other => panic!("expected the discard PayCost prompt, got {other:?}"),
    }
}
