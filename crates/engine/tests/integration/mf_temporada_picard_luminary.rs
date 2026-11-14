//! Temporada 2026-12 da Mesa do MagicFinder — o comandante Jean-Luc Picard,
//! Luminary saía com a lacuna `unrecognized_clause_head`:
//! "Seek out — Whenever you draw your second card each turn, put a +1/+1 counter
//! on up to one other target creature and two charge counters on up to one target
//! Spacecraft."
//!
//! O segundo conjunto ("two charge counters on …") não tem verbo; o parser só
//! herdava o "put" do primeiro quando o conjunto começava com artigo ("and a
//! charge counter on …"). Agora também quando começa com número (CR 122.1).

use engine::game::effects::draw::resolve as resolve_draw;
use engine::game::scenario::{GameRunner, GameScenario, P0};
use engine::game::triggers::process_triggers;
use engine::parser::oracle::parse_oracle_text;
use engine::types::ability::{Effect, QuantityExpr, ResolvedAbility, TargetFilter, TargetRef};
use engine::types::actions::GameAction;
use engine::types::counter::CounterType;
use engine::types::game_state::WaitingFor;
use engine::types::identifiers::ObjectId;
use engine::types::phase::Phase;

const PICARD: &str = "Whenever another historic permanent you control with mana value 3 or \
greater enters, draw a card. (Artifacts, legendaries, and Sagas are historic.)\nSeek out — \
Whenever you draw your second card each turn, put a +1/+1 counter on up to one other target \
creature and two charge counters on up to one target Spacecraft.";

#[test]
fn seek_out_parses_both_counter_conjuncts() {
    let parsed = parse_oracle_text(
        PICARD,
        "Jean-Luc Picard, Luminary",
        &[],
        &["Creature".to_string()],
        &["Human".to_string(), "Officer".to_string()],
    );
    let seek_out = parsed
        .triggers
        .iter()
        .find_map(|t| {
            let exec = t.execute.as_deref()?;
            matches!(&*exec.effect, Effect::PutCounter { .. }).then_some(exec)
        })
        .expect("the Seek out trigger");
    let sub = seek_out
        .sub_ability
        .as_deref()
        .expect("the charge conjunct");
    let Effect::PutCounter {
        counter_type,
        count,
        target,
    } = &*sub.effect
    else {
        panic!("expected the charge PutCounter, got {:?}", sub.effect);
    };
    assert_eq!(counter_type, &CounterType::Generic("charge".to_string()));
    assert_eq!(count, &QuantityExpr::Fixed { value: 2 });
    assert!(matches!(target, TargetFilter::Typed(_)), "{target:?}");
    assert!(
        sub.multi_target.is_some(),
        "\"up to one target Spacecraft\" announces its own optional slot"
    );
}

/// A conjunct that is NOT a counter placement still fails closed.
#[test]
fn a_number_led_non_counter_conjunct_is_not_given_the_put_verb() {
    let parsed = parse_oracle_text(
        "When this creature enters, put a +1/+1 counter on target creature and two \
         creatures can't block this turn.",
        "Probe",
        &[],
        &["Creature".to_string()],
        &[],
    );
    let exec = parsed.triggers[0].execute.as_deref().unwrap();
    let carried_counter = exec.sub_ability.as_deref().is_some_and(|sub| {
        matches!(&*sub.effect, Effect::PutCounter { counter_type, .. }
            if *counter_type != CounterType::Plus1Plus1)
    });
    assert!(!carried_counter, "{:?}", exec.sub_ability);
}

fn draw_one(runner: &mut GameRunner) {
    let ability = ResolvedAbility::new(
        Effect::Draw {
            count: QuantityExpr::Fixed { value: 1 },
            target: TargetFilter::Controller,
        },
        Vec::new(),
        ObjectId(0),
        P0,
    );
    let mut events = Vec::new();
    resolve_draw(runner.state_mut(), &ability, &mut events).expect("draw resolves");
    process_triggers(runner.state_mut(), &events);
}

fn counters(runner: &GameRunner, id: ObjectId, kind: &CounterType) -> u32 {
    runner.state().objects[&id]
        .counters
        .get(kind)
        .copied()
        .unwrap_or(0)
}

/// CR 122.1: the second draw puts a +1/+1 counter on the chosen creature and two
/// charge counters on the chosen Spacecraft.
#[test]
fn second_draw_puts_the_counters_on_both_targets() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let picard = scenario
        .add_creature_from_oracle(P0, "Jean-Luc Picard, Luminary", 3, 3, PICARD)
        .id();
    let bear = scenario.add_creature(P0, "Bear", 2, 2).id();
    let ship = {
        let mut builder = scenario.add_artifact_from_oracle(P0, "Test Ship", "");
        builder.with_subtypes(vec!["Spacecraft"]);
        builder.id()
    };
    for i in 0..4 {
        scenario.add_card_to_library_top(P0, &format!("Library Card {i}"));
    }
    let mut runner = scenario.build();

    draw_one(&mut runner);
    draw_one(&mut runner);
    let mut guard = 0;
    loop {
        runner.advance_until_stack_empty();
        let WaitingFor::TriggerTargetSelection {
            target_slots,
            selection,
            ..
        } = runner.state().waiting_for.clone()
        else {
            break;
        };
        guard += 1;
        assert!(guard < 6, "target selection must end");
        let legal = &target_slots[selection.current_slot].legal_targets;
        assert!(
            !legal.contains(&TargetRef::Object(picard)),
            "\"other target creature\" excludes Picard"
        );
        let pick = [bear, ship]
            .into_iter()
            .map(TargetRef::Object)
            .find(|t| legal.contains(t));
        runner
            .act(GameAction::ChooseTarget { target: pick })
            .expect("target accepted");
    }
    assert!(
        guard > 0,
        "the second draw fires Seek out and asks for its targets"
    );

    assert_eq!(counters(&runner, bear, &CounterType::Plus1Plus1), 1);
    assert_eq!(
        counters(&runner, ship, &CounterType::Generic("charge".to_string())),
        2
    );
    assert_eq!(counters(&runner, ship, &CounterType::Plus1Plus1), 0);
}
