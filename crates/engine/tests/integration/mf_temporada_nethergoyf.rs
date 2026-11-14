//! Temporada 2026-12 da Mesa do MagicFinder — Rakdos Midrange (Modern) recusado
//! porque Nethergoyf saía com `Effect:unknown` + `Misparse:cost_wrong`:
//! "Escape—{2}{B}, Exile any number of other cards from your graveyard with four or
//! more card types among them."
//!
//! O custo de Escape não era lido (a linha inteira virava lacuna). Agora é
//! `AbilityCost::ExileWithAggregate` com a redução de conjunto
//! `AggregateFunction::DistinctCardTypes` (CR 205.2a), paga na conjuração (CR
//! 601.2h + CR 702.138a) pelo mesmo pedido "exile qualquer número" do Baron Helmut
//! Zemo. A P/T (tipos de carta no seu cemitério) já funcionava.

use engine::game::casting::spell_objects_available_to_cast;
use engine::game::scenario::{GameRunner, GameScenario, P0};
use engine::parser::oracle::parse_oracle_text;
use engine::types::ability::{AbilityCost, AggregateFunction, Comparator, Effect};
use engine::types::actions::GameAction;
use engine::types::game_state::{CastPaymentMode, PayCostKind, WaitingFor};
use engine::types::identifiers::ObjectId;
use engine::types::keywords::{EscapeCost, Keyword};
use engine::types::mana::{ManaCost, ManaType, ManaUnit};
use engine::types::phase::Phase;
use engine::types::zones::Zone;

const NETHERGOYF: &str = "Nethergoyf's power is equal to the number of card types among cards \
in your graveyard and its toughness is equal to that number plus 1.\nEscape—{2}{B}, Exile any \
number of other cards from your graveyard with four or more card types among them. (You may \
cast this card from your graveyard for its escape cost.)";

fn mana(color: ManaType, n: usize) -> Vec<ManaUnit> {
    (0..n)
        .map(|_| ManaUnit::new(color, ObjectId(0), false, vec![]))
        .collect()
}

#[test]
fn escape_cost_parses_as_a_distinct_card_types_aggregate() {
    let parsed = parse_oracle_text(
        NETHERGOYF,
        "Nethergoyf",
        &["Escape".to_string()],
        &["Creature".to_string()],
        &["Lhurgoyf".to_string()],
    );
    assert!(
        parsed
            .abilities
            .iter()
            .all(|a| !matches!(&*a.effect, Effect::Unimplemented { .. })),
        "the escape line is no longer a gap: {:?}",
        parsed.abilities
    );
    let escape = parsed
        .extracted_keywords
        .iter()
        .find_map(|k| match k {
            Keyword::Escape(EscapeCost::NonMana(cost)) => Some(cost.clone()),
            _ => None,
        })
        .expect("Escape with a compound cost");
    let AbilityCost::Composite { costs } = escape else {
        panic!("compound escape cost, got {escape:?}");
    };
    assert!(
        costs.iter().any(|c| matches!(
            c,
            AbilityCost::ExileWithAggregate {
                function: AggregateFunction::DistinctCardTypes,
                comparator: Comparator::GE,
                value: 4,
                zone: Zone::Graveyard,
                ..
            }
        )),
        "{costs:?}"
    );
    assert!(costs.iter().any(|c| matches!(c, AbilityCost::Mana { .. })));
}

/// Graveyard: Nethergoyf + `types` other cards, one per listed card type.
fn board(types: &[&str]) -> (GameRunner, ObjectId, Vec<ObjectId>) {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.with_mana_pool(
        P0,
        mana(ManaType::Colorless, 2)
            .into_iter()
            .chain(mana(ManaType::Black, 1))
            .collect(),
    );
    let mut others = Vec::new();
    for ty in types {
        let id = match *ty {
            "land" => scenario.add_land_to_graveyard(P0, "Swamp").id(),
            "artifact" => scenario.add_artifact_to_graveyard(P0, "Bauble").id(),
            "instant" => scenario.add_spell_to_graveyard(P0, "Shock", true).id(),
            "sorcery" => scenario.add_spell_to_graveyard(P0, "Duress", false).id(),
            "creature" => scenario.add_creature_to_graveyard(P0, "Bear", 2, 2).id(),
            other => panic!("unknown type {other}"),
        };
        others.push(id);
    }
    let goyf = {
        let mut builder = scenario.add_creature_to_graveyard(P0, "Nethergoyf", 0, 1);
        builder
            .with_mana_cost(ManaCost::generic(0))
            .from_oracle_text(NETHERGOYF);
        builder.id()
    };
    (scenario.build(), goyf, others)
}

#[test]
fn three_card_types_are_not_enough_to_escape() {
    let (runner, goyf, _) = board(&["land", "instant", "creature"]);
    assert!(!spell_objects_available_to_cast(runner.state(), P0).contains(&goyf));
}

/// CR 702.138a + CR 118.3: with four card types among the other cards the escape is
/// offered; a chosen set short of four types is refused, the full cover pays,
/// the chosen cards are exiled and Nethergoyf resolves onto the battlefield.
#[test]
fn four_card_types_pay_the_escape_and_nethergoyf_enters() {
    let (mut runner, goyf, others) = board(&["land", "instant", "creature", "artifact"]);
    assert!(spell_objects_available_to_cast(runner.state(), P0).contains(&goyf));

    let card_id = runner.state().objects[&goyf].card_id;
    runner
        .act(GameAction::CastSpell {
            object_id: goyf,
            card_id,
            targets: vec![],
            payment_mode: CastPaymentMode::Auto,
        })
        .expect("escape cast enters the pipeline");

    let mut paid = false;
    for _ in 0..16 {
        match runner.state().waiting_for.clone() {
            WaitingFor::PayCost {
                kind: PayCostKind::ExileAggregate { .. },
                choices,
                ..
            } => {
                assert!(!choices.contains(&goyf), "\"other\" cards only");
                assert!(
                    runner
                        .act(GameAction::SelectCards {
                            cards: others[..3].to_vec(),
                        })
                        .is_err(),
                    "three card types do not meet the threshold"
                );
                runner
                    .act(GameAction::SelectCards {
                        cards: others.clone(),
                    })
                    .expect("four card types pay the escape cost");
                paid = true;
            }
            WaitingFor::Priority { .. } => {
                runner.advance_until_stack_empty();
                break;
            }
            other => panic!("unexpected state during the escape cast: {other:?}"),
        }
    }
    assert!(paid, "the aggregate exile prompt must have been answered");
    for id in &others {
        assert_eq!(runner.state().objects[id].zone, Zone::Exile);
    }
    assert_eq!(runner.state().objects[&goyf].zone, Zone::Battlefield);
}
