//! Tyvar the Bellicose grants each creature you control: "Whenever a mana ability
//! of this creature resolves, put a number of +1/+1 counters on it equal to the
//! amount of mana this creature produced. This ability triggers only once each
//! turn."
//!
//! CR 605.1b: the trigger event is the RESOLUTION of a mana ability, tap cost or
//! not (`GameEvent::ManaAbilityProduced`). CR 603.7c: "the amount of mana this
//! creature produced" reads that triggering resolution. CR 201.5a does not apply:
//! "this creature" in the granted body is the host, not Tyvar.

use engine::game::scenario::{GameScenario, P0, P1};
use engine::parser::oracle::parse_oracle_text;
use engine::types::ability::{ContinuousModification, QuantityRef, TargetFilter};
use engine::types::actions::GameAction;
use engine::types::counter::CounterType;
use engine::types::game_state::WaitingFor;
use engine::types::identifiers::ObjectId;
use engine::types::phase::Phase;
use engine::types::triggers::TriggerMode;

const TYVAR: &str = "Whenever one or more Elves you control attack, they gain deathtouch until end of turn.\nEach creature you control has \"Whenever a mana ability of this creature resolves, put a number of +1/+1 counters on it equal to the amount of mana this creature produced. This ability triggers only once each turn.\"";

#[test]
fn granted_trigger_parses_to_mana_ability_produced_on_the_host() {
    let parsed = parse_oracle_text(
        TYVAR,
        "Tyvar the Bellicose",
        &[],
        &["Creature".to_string()],
        &["Elf".to_string(), "Warrior".to_string()],
    );
    let trigger = parsed
        .statics
        .iter()
        .flat_map(|s| s.modifications.iter())
        .find_map(|m| match m {
            ContinuousModification::GrantTrigger { trigger } => Some(trigger),
            _ => None,
        })
        .expect("the static must grant a trigger");

    assert_eq!(trigger.mode, TriggerMode::ManaAbilityProduced);
    assert_eq!(trigger.valid_card, Some(TargetFilter::SelfRef));
    assert!(
        trigger.constraint.is_some(),
        "\"triggers only once each turn\" must survive as a constraint",
    );
    let body = format!("{:?}", trigger.execute);
    assert!(
        body.contains(&format!("{:?}", QuantityRef::EventContextAmount)),
        "the counter count must read the triggering resolution: {body}",
    );
    assert!(
        !body.contains("Unimplemented"),
        "the granted body must parse completely: {body}",
    );
}

/// Tyvar plus one creature of ours whose only ability is `mana_text`.
fn board(mana_text: &str) -> (GameScenario, ObjectId, ObjectId) {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let tyvar = scenario
        .add_creature_from_oracle(P0, "Tyvar the Bellicose", 5, 4, TYVAR)
        .id();
    let elf = scenario
        .add_creature_from_oracle(P0, "Llanowar Elves", 1, 1, mana_text)
        .id();
    (scenario, tyvar, elf)
}

#[test]
fn tapping_a_creature_for_mana_puts_that_many_counters_on_it() {
    let (mut scenario, tyvar_id, _) = board("{T}: Add {G}.");
    let druid = scenario
        .add_creature_from_oracle(P0, "Devoted Druid", 0, 2, "{T}: Add {G}{G}.")
        .id();

    let mut runner = scenario.build();
    runner
        .act(GameAction::ActivateAbility {
            payment_mode: engine::types::game_state::CastPaymentMode::Auto,
            source_id: druid,
            ability_index: 0,
        })
        .expect("the mana ability must activate");
    assert_eq!(
        runner.state().players[0].mana_pool.total(),
        2,
        "the mana ability itself resolves first (CR 605.3b)",
    );
    runner.resolve_top();

    assert_eq!(
        p1p1(&runner, druid),
        2,
        "two mana produced = two +1/+1 counters on the creature that made it",
    );
    assert_eq!(
        p1p1(&runner, tyvar_id),
        0,
        "\"this creature\" is the host of the granted ability, never Tyvar",
    );
}

#[test]
fn non_tap_mana_ability_triggers_and_only_once_each_turn() {
    let (scenario, _, elf) = board("Pay 1 life: Add {G}.");
    let mut runner = scenario.build();

    for _ in 0..2 {
        runner
            .act(GameAction::ActivateAbility {
                payment_mode: engine::types::game_state::CastPaymentMode::Auto,
                source_id: elf,
                ability_index: 0,
            })
            .expect("the mana ability must activate");
        runner.advance_until_stack_empty();
    }

    assert_eq!(
        runner.state().players[0].mana_pool.total(),
        2,
        "both activations produced their mana",
    );
    assert_eq!(
        p1p1(&runner, elf),
        1,
        "the second resolution in the same turn must not trigger again",
    );
}

#[test]
fn opponents_creatures_do_not_have_the_ability() {
    let (mut scenario, _, _) = board("{T}: Add {G}.");
    let theirs = scenario
        .add_creature_from_oracle(P1, "Their Elf", 1, 1, "{T}: Add {G}.")
        .id();
    let mut runner = scenario.build();
    runner.state_mut().priority_player = P1;
    runner.state_mut().waiting_for = WaitingFor::Priority { player: P1 };

    runner
        .act(GameAction::ActivateAbility {
            payment_mode: engine::types::game_state::CastPaymentMode::Auto,
            source_id: theirs,
            ability_index: 0,
        })
        .expect("the opponent's mana ability must activate");
    runner.advance_until_stack_empty();

    assert_eq!(p1p1(&runner, theirs), 0);
}

fn tyvar_face(text: &str) -> engine::types::card::CardFace {
    let card: engine::database::mtgjson::AtomicCard = serde_json::from_value(serde_json::json!({
        "name": "Tyvar the Bellicose",
        "manaCost": "{2}{B}{G}",
        "colors": ["B", "G"],
        "colorIdentity": ["B", "G"],
        "power": "5",
        "toughness": "4",
        "text": text,
        "layout": "normal",
        "type": "Legendary Creature — Elf Warrior",
        "types": ["Creature"],
        "subtypes": ["Elf", "Warrior"],
        "supertypes": ["Legendary"],
        "manaValue": 4.0,
        "identifiers": {}
    }))
    .expect("synthetic MTGJSON card");
    engine::database::synthesis::build_oracle_face(&card, None)
}

#[test]
fn coverage_reports_every_line_supported() {
    fn walk(items: &[engine::game::coverage::ParsedItem], out: &mut Vec<String>) {
        for item in items {
            if !item.supported {
                out.push(format!("{} / {:?}", item.label, item.source_text));
            }
            walk(&item.children, out);
        }
    }
    let details = engine::game::coverage::build_parse_details_for_face(&tyvar_face(TYVAR));
    let mut gaps = Vec::new();
    walk(&details, &mut gaps);
    assert!(gaps.is_empty(), "unsupported parse items: {gaps:?}");
}

fn p1p1(runner: &engine::game::scenario::GameRunner, id: ObjectId) -> u32 {
    runner.state().objects[&id]
        .counters
        .get(&CounterType::Plus1Plus1)
        .copied()
        .unwrap_or(0)
}
