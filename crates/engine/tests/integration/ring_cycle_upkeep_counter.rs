//! Ring of Thune (and the rest of the M13 ring cycle) — "At the beginning of
//! your upkeep, put a +1/+1 counter on equipped creature if it's white."
//!
//! The "if it's white" gate reads the equipped creature (CR 608.2c), which is
//! named by description, not targeted (CR 115.10a). The ability has no object
//! target and the upkeep trigger has no triggering object, so the subject used
//! to be absent and the gate read false forever: the trigger went on the stack
//! every upkeep and resolved without placing a counter.

use engine::game::game_object::AttachTarget;
use engine::game::scenario::{GameScenario, P0};
use engine::types::counter::CounterType;
use engine::types::mana::ManaColor;
use engine::types::phase::Phase;

const RING_OF_THUNE: &str = "Equipped creature has vigilance.\n\
    At the beginning of your upkeep, put a +1/+1 counter on equipped creature if it's white.\n\
    Equip {1}";

/// Counters on the bearer after one upkeep of P0, with the ring attached or not.
fn upkeep_counters(bearer_color: ManaColor, attached: bool) -> u32 {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::Untap);
    let bearer = scenario
        .add_creature(P0, "Bearer", 2, 1)
        .with_color(vec![bearer_color])
        .id();
    let ring = scenario
        .add_artifact_from_oracle(P0, "Ring of Thune", RING_OF_THUNE)
        .with_subtypes(vec!["Equipment"])
        .id();
    let mut runner = scenario.build();
    if attached {
        let state = runner.state_mut();
        state.objects.get_mut(&ring).unwrap().attached_to = Some(AttachTarget::Object(bearer));
        state
            .objects
            .get_mut(&bearer)
            .unwrap()
            .attachments
            .push(ring);
    }
    runner.advance_to_phase(Phase::Upkeep);
    runner.advance_until_stack_empty();
    runner
        .state()
        .objects
        .get(&bearer)
        .and_then(|o| o.counters.get(&CounterType::Plus1Plus1).copied())
        .unwrap_or(0)
}

#[test]
fn white_equipped_creature_gets_a_counter_each_upkeep() {
    assert_eq!(upkeep_counters(ManaColor::White, true), 1);
}

#[test]
fn nonwhite_equipped_creature_gets_nothing() {
    assert_eq!(upkeep_counters(ManaColor::Red, true), 0);
}

#[test]
fn unattached_ring_puts_no_counter_anywhere() {
    assert_eq!(upkeep_counters(ManaColor::White, false), 0);
}
