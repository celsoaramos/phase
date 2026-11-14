//! CR 614.1c + CR 706.1 + CR 706.4: "As this creature enters, roll X d6. It
//! enters with a number of +1/+1 counters on it equal to the total of those
//! results." (Neverwinter Hydra; Hydradoodle spells the dice "six-sided dice"
//! and names itself "This creature".)
//!
//! The line is one replacement: the roll happens as part of the entry event
//! and the counters it produces are placed as the creature enters, so the 0/0
//! never sees state-based actions without them. Before the fix the parser left
//! the line unclaimed and the Hydra entered as a 0/0 and died.

use engine::game::scenario::{GameRunner, GameScenario, P0};
use engine::types::actions::GameAction;
use engine::types::counter::CounterType;
use engine::types::events::GameEvent;
use engine::types::game_state::{CastPaymentMode, WaitingFor};
use engine::types::identifiers::ObjectId;
use engine::types::mana::{ManaCost, ManaCostShard, ManaType, ManaUnit};
use engine::types::phase::Phase;
use engine::types::zones::Zone;

/// Neverwinter Hydra, verbatim (MTGJSON), reminder text included.
const NEVERWINTER_HYDRA: &str = "As this creature enters, roll X d6. It enters with a number of \
+1/+1 counters on it equal to the total of those results.\nTrample\nWard {4} (Whenever this \
creature becomes the target of a spell or ability an opponent controls, counter it unless that \
player pays {4}.)";

/// Hydradoodle, verbatim (MTGJSON).
const HYDRADOODLE: &str = "As this creature enters, roll X six-sided dice. This creature enters \
with a number of +1/+1 counters on it equal to the total of those results.\nReach, trample";

fn act_events(runner: &mut GameRunner, action: GameAction) -> Vec<GameEvent> {
    let label = format!("{action:?}");
    runner
        .act(action)
        .unwrap_or_else(|error| panic!("{label} must be accepted: {error:?}"))
        .events
}

/// Cast `spell` with the given X and pass priority until it has resolved,
/// returning every event emitted on the way.
fn cast_with_x_and_resolve(runner: &mut GameRunner, spell: ObjectId, x: u32) -> Vec<GameEvent> {
    let card_id = runner.state().objects[&spell].card_id;
    let mut events = act_events(
        runner,
        GameAction::CastSpell {
            object_id: spell,
            card_id,
            targets: vec![],
            payment_mode: CastPaymentMode::Auto,
        },
    );
    for _ in 0..16 {
        let action = match &runner.state().waiting_for {
            WaitingFor::ChooseXValue { .. } => GameAction::ChooseX { value: x },
            WaitingFor::ManaPayment { .. } => GameAction::PassPriority,
            WaitingFor::Priority { .. } if runner.state().stack.is_empty() => return events,
            WaitingFor::Priority { .. } => GameAction::PassPriority,
            other => panic!("unexpected prompt while casting: {other:?}"),
        };
        events.extend(act_events(runner, action));
    }
    panic!("the cast must resolve: {:?}", runner.state().waiting_for);
}

fn d6_results(events: &[GameEvent]) -> Vec<u32> {
    events
        .iter()
        .filter_map(|event| match event {
            GameEvent::DieRolled {
                sides: 6,
                result: Some(result),
                ..
            } => Some(u32::from(*result)),
            _ => None,
        })
        .collect()
}

fn p1p1(runner: &GameRunner, object: ObjectId) -> u32 {
    runner.state().objects[&object]
        .counters
        .get(&CounterType::Plus1Plus1)
        .copied()
        .unwrap_or(0)
}

fn cast_hydra(name: &str, oracle: &str, x: u32) -> (GameRunner, ObjectId, Vec<GameEvent>) {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let hydra = scenario
        .add_creature_to_hand_from_oracle(P0, name, 0, 0, oracle)
        .with_mana_cost(ManaCost::Cost {
            shards: vec![
                ManaCostShard::X,
                ManaCostShard::X,
                ManaCostShard::Green,
                ManaCostShard::Green,
            ],
            generic: 0,
        })
        .id();
    let mut runner = scenario.build();
    for _ in 0..(2 * x + 2) {
        runner.state_mut().players[0].mana_pool.add(ManaUnit::new(
            ManaType::Green,
            ObjectId(0),
            false,
            vec![],
        ));
    }
    let events = cast_with_x_and_resolve(&mut runner, hydra, x);
    (runner, hydra, events)
}

/// CR 706.1 + CR 706.4 + CR 614.1c: X = 3 rolls three d6 and the Hydra enters
/// with exactly their total, surviving state-based actions.
#[test]
fn neverwinter_hydra_enters_with_the_total_of_x_d6() {
    let (runner, hydra, events) = cast_hydra("Neverwinter Hydra", NEVERWINTER_HYDRA, 3);
    let rolls = d6_results(&events);
    assert_eq!(rolls.len(), 3, "X = 3 must roll three d6: {rolls:?}");
    let total: u32 = rolls.iter().sum();
    let object = &runner.state().objects[&hydra];
    assert_eq!(
        object.zone,
        Zone::Battlefield,
        "the Hydra must survive with its counters (rolls {rolls:?})"
    );
    assert_eq!(
        p1p1(&runner, hydra),
        total,
        "counters must equal the total of {rolls:?}, not one die or a fixed count"
    );
}

/// Hydradoodle: same shape, "six-sided dice" and "This creature enters".
#[test]
fn hydradoodle_enters_with_the_total_of_x_six_sided_dice() {
    let (runner, hydra, events) = cast_hydra("Hydradoodle", HYDRADOODLE, 4);
    let rolls = d6_results(&events);
    assert_eq!(rolls.len(), 4, "X = 4 must roll four d6: {rolls:?}");
    let total: u32 = rolls.iter().sum();
    assert_eq!(runner.state().objects[&hydra].zone, Zone::Battlefield);
    assert_eq!(p1p1(&runner, hydra), total, "rolls {rolls:?}");
}

/// CR 706.1: X = 0 rolls no dice — the 0/0 enters with no counters and dies
/// to state-based actions (CR 704.5f). No stale die result leaks in.
#[test]
fn neverwinter_hydra_with_x_zero_rolls_nothing_and_dies() {
    let (runner, hydra, events) = cast_hydra("Neverwinter Hydra", NEVERWINTER_HYDRA, 0);
    assert!(d6_results(&events).is_empty(), "X = 0 rolls no dice");
    assert_eq!(runner.state().objects[&hydra].zone, Zone::Graveyard);
}
