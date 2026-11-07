//! Field reports 2026-11-07 (MagicFinder mesa), second batch.
//!
//! 1. Prophet of Kruphix — "Untap all creatures and lands you control during
//!    each other player's untap step." — never untapped anything on the
//!    opponent's turn. "creatures and lands you control" parses to an `Or` of
//!    two typed legs, and the Seedborn-class detector only accepted a single
//!    typed subject (CR 502.3).

use engine::game::scenario::{GameScenario, P0, P1};
use engine::types::mana::ManaColor;
use engine::types::phase::Phase;

const PROPHET: &str = "You may cast creature spells as though they had flash.\n\
Untap all creatures and lands you control during each other player's untap step.";

#[test]
fn prophet_of_kruphix_untaps_creatures_and_lands_on_opponents_untap_step() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::End);
    let prophet = scenario
        .add_creature_from_oracle(P0, "Prophet of Kruphix", 2, 3, PROPHET)
        .id();
    let bear = scenario.add_creature(P0, "Grizzly Bears", 2, 2).id();
    let forest = scenario.add_basic_land(P0, ManaColor::Green);
    let island = scenario.add_basic_land(P0, ManaColor::Blue);
    let mut runner = scenario.build();
    for id in [prophet, bear, forest, island] {
        runner.state_mut().objects.get_mut(&id).unwrap().tapped = true;
    }

    runner.advance_to_upkeep();
    assert_eq!(runner.state().active_player, P1, "reach guard: P1's turn");

    for id in [prophet, bear, forest, island] {
        assert!(
            !runner.state().objects[&id].tapped,
            "CR 502.3: Prophet's controller untaps creatures and lands during each \
             other player's untap step ({})",
            runner.state().objects[&id].name
        );
    }
}

#[test]
fn prophet_of_kruphix_leaves_other_permanent_types_tapped() {
    // Non-vacuity for the compound subject: only creatures and lands untap —
    // an artifact its controller taps stays tapped through the opponent's
    // untap step.
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::End);
    scenario.add_creature_from_oracle(P0, "Prophet of Kruphix", 2, 3, PROPHET);
    let rock = scenario
        .add_artifact_from_oracle(P0, "Inert Relic", "")
        .id();
    let mut runner = scenario.build();
    runner.state_mut().objects.get_mut(&rock).unwrap().tapped = true;

    runner.advance_to_upkeep();
    assert_eq!(runner.state().active_player, P1);
    assert!(
        runner.state().objects[&rock].tapped,
        "Prophet untaps creatures and lands only, not artifacts"
    );
}
