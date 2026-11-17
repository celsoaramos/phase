//! Relatos da mesa de 17/11/2026 (motor v0.125.0).
//!
//! - **Devouring Hellion**: "As this creature enters, you may sacrifice any
//!   number of creatures and/or planeswalkers. If you do, it enters with twice
//!   that many +1/+1 counters on it." — a linha inteira caía no gap
//!   `enters_with_clause` e o Hellion entrava sem perguntar nada.

use engine::game::scenario::{GameRunner, GameScenario, P0};
use engine::types::actions::GameAction;
use engine::types::counter::CounterType;
use engine::types::game_state::WaitingFor;
use engine::types::identifiers::ObjectId;
use engine::types::mana::ManaCost;
use engine::types::phase::Phase;
use engine::types::zones::Zone;

const DEVOURING_HELLION: &str = "As this creature enters, you may sacrifice any number of \
creatures and/or planeswalkers. If you do, it enters with twice that many +1/+1 counters on it.";

fn hellion_scenario() -> (GameScenario, ObjectId, ObjectId, ObjectId) {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let hellion = scenario
        .add_creature_to_hand_from_oracle(P0, "Devouring Hellion", 2, 2, DEVOURING_HELLION)
        .with_mana_cost(ManaCost::zero())
        .id();
    let bear = scenario.add_creature(P0, "Grizzly Bears", 2, 2).id();
    let walker = scenario
        .add_planeswalker_from_oracle(P0, "Chandra, Novice", "Chandra", 3, "")
        .id();
    (scenario, hellion, bear, walker)
}

fn plus_counters(runner: &GameRunner, id: ObjectId) -> u32 {
    runner.state().objects[&id]
        .counters
        .get(&CounterType::Plus1Plus1)
        .copied()
        .unwrap_or(0)
}

#[test]
fn devouring_hellion_enters_with_two_counters_per_permanent_sacrificed() {
    let (scenario, hellion, bear, walker) = hellion_scenario();
    let mut runner = scenario.build();
    runner.cast(hellion).effect_zone(&[bear, walker]).resolve();

    assert_eq!(runner.state().objects[&hellion].zone, Zone::Battlefield);
    assert_eq!(runner.state().objects[&bear].zone, Zone::Graveyard);
    assert_eq!(
        runner.state().objects[&walker].zone,
        Zone::Graveyard,
        "planeswalkers are part of the pool (\"creatures and/or planeswalkers\")"
    );
    assert_eq!(
        plus_counters(&runner, hellion),
        4,
        "two permanents sacrificed → twice that many +1/+1 counters"
    );
}

#[test]
fn devouring_hellion_prompt_excludes_itself_and_decline_enters_without_counters() {
    let (scenario, hellion, bear, walker) = hellion_scenario();
    let mut runner = scenario.build();
    runner.cast(hellion).resolve();

    let WaitingFor::EffectZoneChoice { cards, .. } = &runner.state().waiting_for else {
        panic!(
            "expected the as-enters sacrifice prompt, got {:?}",
            runner.state().waiting_for
        );
    };
    assert!(
        cards.contains(&bear) && cards.contains(&walker),
        "pool={cards:?}"
    );
    assert!(
        !cards.contains(&hellion),
        "the Hellion cannot devour itself; pool={cards:?}"
    );

    runner
        .act(GameAction::SelectCards { cards: vec![] })
        .expect("decline the sacrifice");
    runner.advance_until_stack_empty();

    assert_eq!(runner.state().objects[&hellion].zone, Zone::Battlefield);
    assert_eq!(runner.state().objects[&bear].zone, Zone::Battlefield);
    assert_eq!(runner.state().objects[&walker].zone, Zone::Battlefield);
    assert_eq!(plus_counters(&runner, hellion), 0);
}
