//! Field reports (MagicFinder, 2026-09-24): alternative-cost casts that
//! misbehaved at the table.
//!
//! - Ensnare: "return two Islands you control to their owner's hand" parsed as
//!   an effect-as-cost that the spell-cost payer never performs, so the spell
//!   resolved for free and the Islands stayed on the battlefield.
//! - Surge: cast for the surge cost after another spell this turn.

use engine::game::scenario::{GameScenario, P0, P1};
use engine::parser::oracle_cost::parse_oracle_cost;
use engine::types::ability::{AbilityCost, SpellCastingOption};
use engine::types::actions::GameAction;
use engine::types::game_state::{CastPaymentMode, CastingVariant, PayCostKind, WaitingFor};
use engine::types::mana::ManaColor;
use engine::types::phase::Phase;
use engine::types::zones::Zone;

const ENSNARE: &str = "You may return two Islands you control to their owner's hand rather than pay this spell's mana cost.\nTap all creatures.";

/// CR 118.3 + CR 107.1a: the count word and the plural destination both belong
/// to a return-to-hand cost.
#[test]
fn return_n_permanents_to_their_owners_hand_is_a_counted_return_cost() {
    match parse_oracle_cost("Return two Islands you control to their owner's hand") {
        AbilityCost::ReturnToHand {
            count: 2,
            filter: Some(_),
            from_zone: None,
        } => {}
        other => panic!("expected ReturnToHand x2, got {other:?}"),
    }
    match parse_oracle_cost("Return three Islands you control to their owners' hands") {
        AbilityCost::ReturnToHand { count: 3, .. } => {}
        other => panic!("expected ReturnToHand x3, got {other:?}"),
    }
    // The singular form keeps its count of one (Daze).
    match parse_oracle_cost("Return an Island you control to its owner's hand") {
        AbilityCost::ReturnToHand { count: 1, .. } => {}
        other => panic!("expected ReturnToHand x1, got {other:?}"),
    }
}

/// CR 118.9: Ensnare's alternative cost returns exactly two Islands, and the
/// spell resolves only after they are back in hand.
#[test]
fn ensnare_alternative_cost_returns_two_islands() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let ensnare = scenario
        .add_spell_to_hand_from_oracle(P0, "Ensnare", true, ENSNARE)
        .id();
    let island_a = scenario.add_basic_land(P0, ManaColor::Blue);
    let island_b = scenario.add_basic_land(P0, ManaColor::Blue);
    let bear = scenario.add_creature(P1, "Bear", 2, 2).id();
    let mut runner = scenario.build();

    let card_id = runner.state().objects[&ensnare].card_id;
    runner
        .act(GameAction::CastSpell {
            object_id: ensnare,
            card_id,
            targets: vec![],
            payment_mode: CastPaymentMode::Auto,
        })
        .expect("cast Ensnare");
    assert!(
        matches!(
            runner.state().waiting_for,
            WaitingFor::OptionalCostChoice { .. }
        ),
        "expected the alternative-cost offer, got {:?}",
        runner.state().waiting_for
    );
    runner
        .act(GameAction::DecideOptionalCost { pay: true })
        .expect("take the alternative cost");
    match &runner.state().waiting_for {
        WaitingFor::PayCost {
            kind: PayCostKind::ReturnToHand,
            count: 2,
            choices,
            ..
        } => {
            assert!(choices.contains(&island_a) && choices.contains(&island_b));
        }
        other => panic!("expected a two-Island return payment, got {other:?}"),
    }
    runner
        .act(GameAction::SelectCards {
            cards: vec![island_a, island_b],
        })
        .expect("return both Islands");
    runner.advance_until_stack_empty();

    for island in [island_a, island_b] {
        assert_eq!(runner.state().objects[&island].zone, Zone::Hand);
    }
    assert!(
        runner.state().objects[&bear].tapped,
        "Ensnare taps all creatures"
    );
}

/// CR 118.9 + CR 601.2b: with only one Island the alternative cost is not
/// payable, so it is not offered.
#[test]
fn ensnare_alternative_cost_needs_two_islands() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let ensnare = scenario
        .add_spell_to_hand_from_oracle(P0, "Ensnare", true, ENSNARE)
        .id();
    scenario.add_basic_land(P0, ManaColor::Blue);
    let mut runner = scenario.build();

    let card_id = runner.state().objects[&ensnare].card_id;
    let result = runner.act(GameAction::CastSpell {
        object_id: ensnare,
        card_id,
        targets: vec![],
        payment_mode: CastPaymentMode::Auto,
    });
    assert!(
        result.is_err()
            || !matches!(
                runner.state().waiting_for,
                WaitingFor::OptionalCostChoice { .. }
            ),
        "one Island must not pay Ensnare's alternative cost"
    );
}

const BOULDER_SALVO: &str = "Surge {1}{R} (You may cast this spell for its surge cost if you or a teammate has cast another spell this turn.)\nBoulder Salvo deals 4 damage to target creature.";

/// A board with `mountains` Mountains, a one-mana instant and Boulder Salvo
/// ({4}{R}, surge {1}{R}) in hand, and an opposing 2/2.
fn surge_scenario(
    mountains: usize,
) -> (
    engine::game::scenario::GameRunner,
    engine::types::identifiers::ObjectId,
    engine::types::identifiers::ObjectId,
    engine::types::identifiers::ObjectId,
) {
    use engine::types::mana::{ManaCost, ManaCostShard};
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let salve = scenario
        .add_spell_to_hand_from_oracle(P0, "Healing Salve", true, "You gain 1 life.")
        .id();
    for _ in 0..mountains {
        scenario.add_basic_land(P0, ManaColor::Red);
    }
    let salvo = scenario
        .add_spell_to_hand_from_oracle(P0, "Boulder Salvo", false, BOULDER_SALVO)
        .id();
    let bear = scenario.add_creature(P1, "Bear", 2, 2).id();
    let mut runner = scenario.build();
    let s = runner.state_mut();
    s.objects.get_mut(&salve).unwrap().mana_cost = ManaCost::generic(1);
    s.objects.get_mut(&salvo).unwrap().mana_cost = ManaCost::Cost {
        shards: vec![ManaCostShard::Red],
        generic: 4,
    };
    (runner, salve, salvo, bear)
}

fn cast_action(
    runner: &engine::game::scenario::GameRunner,
    object_id: engine::types::identifiers::ObjectId,
) -> GameAction {
    GameAction::CastSpell {
        object_id,
        card_id: runner.state().objects[&object_id].card_id,
        targets: vec![],
        payment_mode: CastPaymentMode::Auto,
    }
}

/// CR 702.117a: after another spell this turn, a surge card whose printed cost
/// is out of reach is cast for its surge cost (field report: "surge can't be
/// used even after casting another spell").
#[test]
fn surge_is_castable_after_another_spell() {
    let (mut runner, salve, salvo, bear) = surge_scenario(3);
    runner.cast(salve).resolve();

    runner.cast(salvo).target_object(bear).resolve();

    assert_eq!(
        runner.state().objects[&bear].zone,
        Zone::Graveyard,
        "Boulder Salvo cast for its surge cost kills the Bear"
    );
    let record = runner.state().spells_cast_this_turn_by_player[&P0]
        .iter()
        .find(|r| r.name == "Boulder Salvo")
        .expect("Boulder Salvo was cast");
    assert_eq!(record.cast_variant, CastingVariant::Surge);
}

/// CR 702.117a: surge is not available before another spell was cast.
#[test]
fn surge_needs_another_spell_first() {
    let (mut runner, _salve, salvo, _bear) = surge_scenario(3);
    let action = cast_action(&runner, salvo);
    assert!(
        runner.act(action).is_err(),
        "with no other spell this turn the {{4}}{{R}} printed cost is the only option"
    );
}

/// CR 702.117a + CR 118.9: when both costs are affordable, the caster chooses.
#[test]
fn surge_offers_the_choice_when_both_costs_are_affordable() {
    let (mut runner, salve, salvo, _bear) = surge_scenario(6);
    runner.cast(salve).resolve();

    let action = cast_action(&runner, salvo);
    runner.act(action).expect("cast Boulder Salvo");
    assert!(
        matches!(
            runner.state().waiting_for,
            WaitingFor::AlternativeCastChoice {
                keyword: engine::types::game_state::AlternativeCastKeyword::Surge,
                ..
            }
        ),
        "expected the normal-vs-surge choice, got {}",
        runner.waiting_for_kind()
    );
}
