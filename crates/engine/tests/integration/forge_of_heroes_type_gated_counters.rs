//! CR 122.1 + CR 608.2c: Forge of Heroes — "{T}: Choose target commander that
//! entered this turn. Put a +1/+1 counter on it if it's a creature and a
//! loyalty counter on it if it's a planeswalker."
//!
//! Each placement is gated on its own type: a creature commander gets only the
//! +1/+1 counter, a planeswalker commander only the loyalty counter.

use engine::game::scenario::GameScenario;
use engine::types::counter::CounterType;
use engine::types::phase::Phase;
use engine::types::player::PlayerId;
use engine::types::ObjectId;

const FORGE: &str = "{T}: Add {C}.\n{T}: Choose target commander that entered this turn. \
Put a +1/+1 counter on it if it's a creature and a loyalty counter on it if it's a planeswalker.";

const P0: PlayerId = PlayerId(0);

/// Both commanders enter this turn (`with_summoning_sickness` sets the
/// entered-this-turn turn number the target filter reads).
fn counters_after_forge(planeswalker: bool) -> (u32, u32) {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let forge = scenario
        .add_land_from_oracle(P0, "Forge of Heroes", FORGE)
        .id();
    let commander = if planeswalker {
        scenario
            .add_planeswalker_from_oracle(P0, "Commander Walker", "Ajani", 3, "")
            .commander()
            .with_summoning_sickness()
            .id()
    } else {
        scenario
            .add_creature(P0, "Commander Bear", 2, 2)
            .commander()
            .with_summoning_sickness()
            .id()
    };
    let mut runner = scenario.build();
    let loyalty_before = counter(&runner, commander, CounterType::Loyalty);
    // The activation driver resolves the whole stack and is the same in every
    // engine version this branch is integrated onto.
    runner.activate(forge, 1).target_object(commander).resolve();
    assert!(
        runner.state().stack.is_empty(),
        "the ability resolved and left the stack"
    );
    (
        counter(&runner, commander, CounterType::Plus1Plus1),
        counter(&runner, commander, CounterType::Loyalty) - loyalty_before,
    )
}

fn counter(runner: &engine::game::scenario::GameRunner, id: ObjectId, kind: CounterType) -> u32 {
    runner.state().objects[&id]
        .counters
        .get(&kind)
        .copied()
        .unwrap_or(0)
}

#[test]
fn forge_of_heroes_puts_plus_one_on_a_creature_commander() {
    assert_eq!(
        counters_after_forge(false),
        (1, 0),
        "a creature commander gets the +1/+1 counter and no loyalty counter"
    );
}

#[test]
fn forge_of_heroes_puts_loyalty_on_a_planeswalker_commander() {
    assert_eq!(
        counters_after_forge(true),
        (0, 1),
        "a planeswalker commander gets one loyalty counter and no +1/+1 counter"
    );
}
