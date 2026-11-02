//! Relatos de 02/11/2026 (MagicFinder).
//!
//! Botanical Brawler: "Whenever one or more +1/+1 counters are put on another
//! permanent you control, if it's the first time +1/+1 counters have been put on
//! that permanent this turn, put a +1/+1 counter on Botanical Brawler." Mikaeus,
//! the Lunarch entered with +1/+1 counters and the Brawler got nothing.

use engine::game::scenario::{GameRunner, GameScenario, P0};
use engine::parser::parse_oracle_text;
use engine::types::ability::{Effect, TriggerCondition};
use engine::types::counter::CounterType;
use engine::types::identifiers::ObjectId;
use engine::types::mana::ManaCost;
use engine::types::phase::Phase;

const BOTANICAL_BRAWLER: &str = "Trample\n\
    Botanical Brawler enters with two +1/+1 counters on it.\n\
    Whenever one or more +1/+1 counters are put on another permanent you control, if it's the \
    first time +1/+1 counters have been put on that permanent this turn, put a +1/+1 counter on \
    Botanical Brawler.";

const PUT_PLUS_ONE: &str = "Put a +1/+1 counter on target creature.";
const PUT_MINUS_ONE: &str = "Put a -1/-1 counter on target creature.";
const ENTERS_WITH_TWO: &str = "This creature enters with two +1/+1 counters on it.";

fn p1p1(runner: &GameRunner, id: ObjectId) -> u32 {
    runner
        .state()
        .objects
        .get(&id)
        .and_then(|o| o.counters.get(&CounterType::Plus1Plus1).copied())
        .unwrap_or(0)
}

/// Parse half: the kind-qualified intervening-if ("first time +1/+1 counters")
/// is read. Before, the clause fell through and the whole reproduction lowered to
/// `Effect::Unimplemented`, so the trigger fired and did nothing.
#[test]
fn botanical_brawler_condition_is_first_time_plus_one_counters() {
    let parsed = parse_oracle_text(
        BOTANICAL_BRAWLER,
        "Botanical Brawler",
        &["Trample".to_string()],
        &["Creature".to_string()],
        &["Elemental".to_string(), "Warrior".to_string()],
    );
    let trigger = parsed
        .triggers
        .iter()
        .find(|t| t.condition.is_some())
        .expect("the counter trigger carries the intervening-if");
    assert_eq!(
        trigger.condition,
        Some(
            TriggerCondition::FirstTimeObjectCountersOfTypeAddedThisTurn {
                counter_type: CounterType::Plus1Plus1,
            }
        )
    );
    assert!(
        matches!(
            trigger.execute.as_deref().map(|e| &*e.effect),
            Some(Effect::PutCounter { .. })
        ),
        "reproduction is the +1/+1 counter, not Unimplemented: {:?}",
        trigger.execute
    );
}

/// The kind-less form (Stalwart Successor) keeps its own condition.
#[test]
fn kindless_first_time_counters_keeps_the_any_kind_condition() {
    let parsed = parse_oracle_text(
        "Whenever one or more counters are put on a creature you control, if it's the first time \
         counters have been put on that creature this turn, put a +1/+1 counter on that creature.",
        "Probe",
        &[],
        &["Creature".to_string()],
        &[],
    );
    assert_eq!(
        parsed.triggers[0].condition,
        Some(TriggerCondition::FirstTimeObjectCountersAddedThisTurn)
    );
}

#[test]
fn botanical_brawler_counter_spell_on_other_creature() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let brawler = scenario
        .add_creature_from_oracle(P0, "Botanical Brawler", 2, 2, BOTANICAL_BRAWLER)
        .id();
    let bear = scenario.add_creature(P0, "Grizzly Bears", 2, 2).id();
    let s1 = scenario
        .add_spell_to_hand_from_oracle(P0, "Bolster One", true, PUT_PLUS_ONE)
        .with_mana_cost(ManaCost::generic(0))
        .id();
    let s2 = scenario
        .add_spell_to_hand_from_oracle(P0, "Bolster Two", true, PUT_PLUS_ONE)
        .with_mana_cost(ManaCost::generic(0))
        .id();
    let mut runner = scenario.build();
    let before = p1p1(&runner, brawler);

    runner.cast(s1).target_object(bear).resolve();
    assert_eq!(p1p1(&runner, bear), 1);
    assert_eq!(
        p1p1(&runner, brawler),
        before + 1,
        "first counter on the bear"
    );

    runner.cast(s2).target_object(bear).resolve();
    assert_eq!(p1p1(&runner, bear), 2);
    assert_eq!(
        p1p1(&runner, brawler),
        before + 1,
        "second counter same turn is gated"
    );
}

#[test]
fn botanical_brawler_creature_entering_with_counters() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let brawler = scenario
        .add_creature_from_oracle(P0, "Botanical Brawler", 2, 2, BOTANICAL_BRAWLER)
        .id();
    let entering = scenario
        .add_creature_to_hand_from_oracle(P0, "Walking Ballista Lite", 0, 0, ENTERS_WITH_TWO)
        .with_mana_cost(ManaCost::generic(0))
        .id();
    let mut runner = scenario.build();
    let before = p1p1(&runner, brawler);

    runner.cast(entering).resolve();
    assert_eq!(
        p1p1(&runner, entering),
        2,
        "reach-guard: the creature entered with counters"
    );
    assert_eq!(
        p1p1(&runner, brawler),
        before + 1,
        "a permanent entering with +1/+1 counters had counters put on it (CR 122.6)"
    );
}

/// Kind discriminator: a -1/-1 counter put on the bear earlier this turn does
/// not spend the "first time +1/+1 counters" — the first +1/+1 still fires.
#[test]
fn botanical_brawler_other_kind_first_does_not_spend_the_first_time() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let brawler = scenario
        .add_creature_from_oracle(P0, "Botanical Brawler", 2, 2, BOTANICAL_BRAWLER)
        .id();
    let bear = scenario.add_creature(P0, "Hill Giant", 3, 3).id();
    let wither = scenario
        .add_spell_to_hand_from_oracle(P0, "Shrink", true, PUT_MINUS_ONE)
        .with_mana_cost(ManaCost::generic(0))
        .id();
    let grow = scenario
        .add_spell_to_hand_from_oracle(P0, "Bolster", true, PUT_PLUS_ONE)
        .with_mana_cost(ManaCost::generic(0))
        .id();
    let mut runner = scenario.build();
    let before = p1p1(&runner, brawler);

    runner.cast(wither).target_object(bear).resolve();
    assert_eq!(
        p1p1(&runner, brawler),
        before,
        "-1/-1 counter is not a +1/+1 placement"
    );

    runner.cast(grow).target_object(bear).resolve();
    assert_eq!(
        p1p1(&runner, brawler),
        before + 1,
        "first +1/+1 placement this turn fires even after a -1/-1 placement"
    );
}
