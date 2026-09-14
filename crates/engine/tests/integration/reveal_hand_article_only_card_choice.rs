//! CR 701.20a + CR 608.2d + CR 202.3: "You choose a card from it with mana value N or greater" — the article-only object keeps its restriction (Pelakka Predation class).

use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::parser::oracle_effect::parse_effect_chain;
use engine::types::ability::{
    AbilityDefinition, AbilityKind, Comparator, ControllerRef, Effect, FilterProp, QuantityExpr,
    TargetFilter, TypeFilter,
};
use engine::types::actions::GameAction;
use engine::types::game_state::WaitingFor;
use engine::types::identifiers::ObjectId;
use engine::types::mana::{ManaCost, ManaCostShard, ManaType, ManaUnit};
use engine::types::phase::Phase;
use engine::types::zones::Zone;

const PELAKKA: &str = "Target opponent reveals their hand. You choose a card from it with mana value 3 or greater. That player discards that card.";
const APPETITE: &str = "Target opponent reveals their hand. You choose a card from it with mana value 4 or greater and exile that card.";
const TRANSGRESS: &str = "Devoid (This card has no color.)\nTarget player reveals their hand. You choose a card from it with mana value 3 or greater and exile that card.";
const TRANSGRESS_SPELL_LINE: &str = "Target player reveals their hand. You choose a card from it with mana value 3 or greater and exile that card.";
const COERCION: &str = "Target opponent reveals their hand. You choose a card from it. That player discards that card.";
const DREAD_FUGUE_BASE: &str = "Target player reveals their hand. You choose a nonland card from it with mana value 2 or less. That player discards that card.";
// SYNTHETIC in-class sibling (no printed card): the optional-choice arm.
const SYNTHETIC_MAY_CHOOSE: &str = "Target opponent reveals their hand. You may choose a card from it with mana value 3 or greater. That player discards that card.";

fn reveal_hand_filter(def: &AbilityDefinition) -> Option<&TargetFilter> {
    match def.effect.as_ref() {
        Effect::RevealHand { card_filter, .. } => Some(card_filter),
        _ => def.sub_ability.as_deref().and_then(reveal_hand_filter),
    }
}

fn mana_value_at_least(filter: &TargetFilter, threshold: i32) -> bool {
    matches!(
        filter,
        TargetFilter::Typed(tf)
            if tf.type_filters.is_empty()
                && tf.properties
                    == [FilterProp::Cmc {
                        comparator: Comparator::GE,
                        value: QuantityExpr::Fixed { value: threshold },
                    }]
    )
}

/// How the spell under test is built.
enum SpellText {
    /// A plain sorcery with mana cost {2}{B}.
    Plain(&'static str),
    /// Transgress the Mind: {1}{B}, with the Devoid keyword line passed as a hint.
    Devoid(&'static str),
}

struct Table {
    runner: GameRunner,
    spell: ObjectId,
    opponent_cards: Vec<ObjectId>,
    own_five: ObjectId,
}

/// P0 in main phase with six black mana, the spell and an MV-5 card in hand;
/// P1's hand holds one creature per `(name, mana value)` entry.
fn setup(name: &str, text: SpellText, opponent_hand: &[(&str, u32)]) -> Table {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.with_mana_pool(
        P0,
        (0..6)
            .map(|_| ManaUnit::new(ManaType::Black, ObjectId(0), false, vec![]))
            .collect(),
    );
    let spell = match text {
        SpellText::Plain(oracle) => {
            let mut builder = scenario.add_spell_to_hand_from_oracle(P0, name, false, oracle);
            builder.with_mana_cost(ManaCost::Cost {
                shards: vec![ManaCostShard::Black],
                generic: 2,
            });
            builder.id()
        }
        SpellText::Devoid(oracle) => {
            let mut builder = scenario.add_spell_to_hand(P0, name, false);
            builder.with_mana_cost(ManaCost::Cost {
                shards: vec![ManaCostShard::Black],
                generic: 1,
            });
            builder.from_oracle_text_with_keywords(&["Devoid"], oracle);
            builder.id()
        }
    };
    let opponent_cards = opponent_hand
        .iter()
        .map(|(card_name, mana_value)| {
            let mut builder = scenario.add_creature_to_hand(P1, card_name, 1, 1);
            builder.with_mana_cost(ManaCost::Cost {
                shards: vec![],
                generic: *mana_value,
            });
            builder.id()
        })
        .collect();
    let mut own = scenario.add_creature_to_hand(P0, "Own Five", 1, 1);
    own.with_mana_cost(ManaCost::Cost {
        shards: vec![],
        generic: 5,
    });
    let own_five = own.id();
    Table {
        runner: scenario.build(),
        spell,
        opponent_cards,
        own_five,
    }
}

/// Cast the spell at P1 and return the choice the resolution halted on.
fn cast_and_take_choice(table: &mut Table) -> WaitingFor {
    let outcome = table.runner.cast(table.spell).target_player(P1).resolve();
    outcome.final_waiting_for().clone()
}

fn sorted(mut ids: Vec<ObjectId>) -> Vec<ObjectId> {
    ids.sort();
    ids
}

#[test]
fn article_only_choice_chain_keeps_mana_value_restriction() {
    for (text, threshold) in [(PELAKKA, 3), (APPETITE, 4), (TRANSGRESS_SPELL_LINE, 3)] {
        let def = parse_effect_chain(text, AbilityKind::Spell);
        let Effect::RevealHand {
            target,
            card_filter,
            choice_optional,
            ..
        } = def.effect.as_ref()
        else {
            panic!("expected RevealHand for {text:?}, got {:?}", def.effect);
        };
        // CR 608.2d + CR 202.3: the chosen card is any card with the stated mana value.
        assert!(
            mana_value_at_least(card_filter, threshold),
            "expected an MV >= {threshold} filter with no type constraint for {text:?}, got {card_filter:?}"
        );
        assert!(!choice_optional, "\"You choose\" is mandatory: {text:?}");
        let sub = def
            .sub_ability
            .as_ref()
            .unwrap_or_else(|| panic!("continuation missing for {text:?}"));
        if text == PELAKKA {
            assert!(
                matches!(
                    target,
                    TargetFilter::Typed(tf) if tf.controller == Some(ControllerRef::Opponent)
                ),
                "Pelakka targets an opponent, got {target:?}"
            );
            // CR 701.9a: that player discards the chosen card.
            assert!(
                matches!(
                    sub.effect.as_ref(),
                    Effect::DiscardCard {
                        target: TargetFilter::ParentTarget,
                        ..
                    }
                ),
                "expected a ParentTarget discard, got {:?}",
                sub.effect
            );
        } else {
            if text == TRANSGRESS_SPELL_LINE {
                assert_eq!(target, &TargetFilter::Player);
            } else {
                assert!(
                    matches!(
                        target,
                        TargetFilter::Typed(tf) if tf.controller == Some(ControllerRef::Opponent)
                    ),
                    "Appetite targets an opponent, got {target:?}"
                );
            }
            // CR 701.13a: exile the chosen card.
            assert!(
                matches!(
                    sub.effect.as_ref(),
                    Effect::ChangeZone {
                        destination: Zone::Exile,
                        target: TargetFilter::ParentTarget,
                        ..
                    }
                ),
                "expected a ParentTarget exile, got {:?}",
                sub.effect
            );
        }
    }

    // CR 608.2d: "you may choose" keeps the same restriction and makes the choice optional.
    let may = parse_effect_chain(SYNTHETIC_MAY_CHOOSE, AbilityKind::Spell);
    let Effect::RevealHand {
        card_filter,
        choice_optional,
        ..
    } = may.effect.as_ref()
    else {
        panic!("expected RevealHand, got {:?}", may.effect);
    };
    assert!(
        mana_value_at_least(card_filter, 3),
        "expected an MV >= 3 filter, got {card_filter:?}"
    );
    assert!(*choice_optional);

    // No descriptor and no restriction: any card.
    let coercion = parse_effect_chain(COERCION, AbilityKind::Spell);
    assert_eq!(reveal_hand_filter(&coercion), Some(&TargetFilter::Any));

    // A descriptor with a restriction keeps both.
    let fugue = parse_effect_chain(DREAD_FUGUE_BASE, AbilityKind::Spell);
    let fugue_filter = reveal_hand_filter(&fugue).expect("RevealHand in Dread Fugue chain");
    assert!(
        matches!(
            fugue_filter,
            TargetFilter::Typed(tf)
                if tf.type_filters == [TypeFilter::Non(Box::new(TypeFilter::Land))]
                    && tf.properties
                        == [FilterProp::Cmc {
                            comparator: Comparator::LE,
                            value: QuantityExpr::Fixed { value: 2 },
                        }]
        ),
        "expected nonland + MV <= 2, got {fugue_filter:?}"
    );
}

#[test]
fn pelakka_predation_offers_only_mv3_plus_and_discards_choice() {
    let mut table = setup(
        "Pelakka Predation",
        SpellText::Plain(PELAKKA),
        &[("Small", 1), ("Mid", 3), ("Big", 4)],
    );
    let (small, mid, big) = (
        table.opponent_cards[0],
        table.opponent_cards[1],
        table.opponent_cards[2],
    );

    let waiting = cast_and_take_choice(&mut table);
    // CR 608.2d + CR 202.3: the controller chooses among the MV >= 3 cards only.
    let WaitingFor::RevealChoice { player, cards, .. } = &waiting else {
        panic!("expected RevealChoice, got {waiting:?}");
    };
    assert_eq!(*player, P0);
    assert_eq!(sorted(cards.clone()), sorted(vec![mid, big]));
    assert!(!cards.contains(&small));
    assert!(!cards.contains(&table.own_five));

    table
        .runner
        .act(GameAction::SelectCards { cards: vec![big] })
        .expect("choose the MV-4 card");
    table.runner.advance_until_stack_empty();

    let state = table.runner.state();
    // CR 701.9a + CR 701.9b: the chosen card goes to its owner's graveyard, chosen by P0.
    assert_eq!(state.objects[&big].zone, Zone::Graveyard);
    assert_eq!(state.objects[&big].owner, P1);
    assert_eq!(state.objects[&small].zone, Zone::Hand);
    assert_eq!(state.objects[&mid].zone, Zone::Hand);
    assert_eq!(state.objects[&table.own_five].zone, Zone::Hand);
    assert_eq!(state.players[P1.0 as usize].hand.len(), 2);
}

#[test]
fn appetite_for_brains_exiles_only_mv4_plus() {
    let mut table = setup(
        "Appetite for Brains",
        SpellText::Plain(APPETITE),
        &[("Small", 1), ("Mid", 3), ("Big", 4)],
    );
    let (small, mid, big) = (
        table.opponent_cards[0],
        table.opponent_cards[1],
        table.opponent_cards[2],
    );

    let waiting = cast_and_take_choice(&mut table);
    // CR 202.3: MV 3 is below the threshold of 4.
    let WaitingFor::RevealChoice { player, cards, .. } = &waiting else {
        panic!("expected RevealChoice, got {waiting:?}");
    };
    assert_eq!(*player, P0);
    assert_eq!(cards, &vec![big]);
    assert!(!cards.contains(&mid));

    table
        .runner
        .act(GameAction::SelectCards { cards: vec![big] })
        .expect("choose the MV-4 card");
    table.runner.advance_until_stack_empty();

    let state = table.runner.state();
    // CR 701.13a: the chosen card is exiled.
    assert_eq!(state.objects[&big].zone, Zone::Exile);
    assert_eq!(state.objects[&small].zone, Zone::Hand);
    assert_eq!(state.objects[&mid].zone, Zone::Hand);
}

#[test]
fn transgress_the_mind_exiles_chosen_mv3_plus() {
    let mut table = setup(
        "Transgress the Mind",
        SpellText::Devoid(TRANSGRESS),
        &[("Small", 1), ("Mid", 3), ("Big", 4)],
    );
    let (small, mid, big) = (
        table.opponent_cards[0],
        table.opponent_cards[1],
        table.opponent_cards[2],
    );

    let waiting = cast_and_take_choice(&mut table);
    // CR 608.2d + CR 202.3: both MV >= 3 cards are offered.
    let WaitingFor::RevealChoice { player, cards, .. } = &waiting else {
        panic!("expected RevealChoice, got {waiting:?}");
    };
    assert_eq!(*player, P0);
    assert_eq!(sorted(cards.clone()), sorted(vec![mid, big]));

    table
        .runner
        .act(GameAction::SelectCards { cards: vec![mid] })
        .expect("choose the MV-3 card");
    table.runner.advance_until_stack_empty();

    let state = table.runner.state();
    // CR 701.13a: only the chosen card is exiled.
    assert_eq!(state.objects[&mid].zone, Zone::Exile);
    assert_eq!(state.objects[&small].zone, Zone::Hand);
    assert_eq!(state.objects[&big].zone, Zone::Hand);
}

#[test]
fn pelakka_predation_with_no_eligible_card_discards_nothing() {
    let mut table = setup(
        "Pelakka Predation",
        SpellText::Plain(PELAKKA),
        &[("Small", 1), ("Two", 2)],
    );
    let (small, two) = (table.opponent_cards[0], table.opponent_cards[1]);

    let outcome = table.runner.cast(table.spell).target_player(P1).resolve();
    // CR 608.2d: with no legal choice nothing is chosen, so no choice prompt appears.
    assert!(
        matches!(outcome.final_waiting_for(), WaitingFor::Priority { player } if *player == P0),
        "expected P0 priority, got {:?}",
        outcome.final_waiting_for()
    );
    // CR 701.20a + CR 701.20b: the hand is still revealed, and revealed cards stay in hand.
    assert!(outcome.state().revealed_cards.contains(&small));
    assert!(outcome.state().revealed_cards.contains(&two));
    assert_eq!(outcome.zone_of(table.spell), Zone::Graveyard);
    assert_eq!(outcome.zone_of(small), Zone::Hand);
    assert_eq!(outcome.zone_of(two), Zone::Hand);
    assert_eq!(outcome.zone_of(table.own_five), Zone::Hand);
}
