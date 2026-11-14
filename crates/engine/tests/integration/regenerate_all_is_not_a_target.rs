//! CR 115.1 + CR 701.19a: "Regenerate all Werewolf creatures you control"
//! (Full Moon's Rise) shields EVERY matching permanent and asks nothing.
//!
//! Field report (2026-09-21): the ability opened a target prompt listing the
//! three Werewolves on the battlefield and shielded only the one picked. The
//! card prints no "target": the filter is a population scan, so claiming a
//! target both invented a prompt and threw away the other Werewolves.
//!
//! The runtime half is the discriminating one — a parse assertion alone cannot
//! tell "shielded all of them" from "shielded the first". Here a sweeper
//! destroys the board and the Werewolves regenerate while the plain bear dies.

use engine::game::scenario::{GameScenario, P0};
use engine::parser::oracle_effect::parse_effect_chain;
use engine::types::ability::{AbilityKind, Effect, EffectScope, TargetFilter};
use engine::types::phase::Phase;
use engine::types::zones::Zone;

const FULL_MOONS_RISE: &str =
    "Sacrifice this enchantment: Regenerate all Werewolf creatures you control.";
const SWEEPER: &str = "Destroy all creatures.";

#[test]
fn regenerate_all_parses_as_a_mass_effect_with_no_target() {
    let parsed = parse_effect_chain(
        "Regenerate all Werewolf creatures you control.",
        AbilityKind::Spell,
    );
    assert!(
        matches!(
            parsed.effect.as_ref(),
            Effect::Regenerate {
                scope: EffectScope::All,
                ..
            }
        ),
        "expected a mass regenerate, got {:?}",
        parsed.effect
    );
    assert!(
        parsed.effect.target_filter().is_none(),
        "\"all\" does not target (CR 115.1), so no slot may be declared"
    );

    // The single-permanent sibling keeps its target: the scope axis must not
    // swallow the printed "target".
    let single = parse_effect_chain("Regenerate target creature.", AbilityKind::Spell);
    assert!(
        matches!(
            single.effect.as_ref(),
            Effect::Regenerate {
                scope: EffectScope::Single,
                ..
            }
        ),
        "expected a single-target regenerate, got {:?}",
        single.effect
    );
    assert!(
        matches!(
            single.effect.target_filter(),
            Some(TargetFilter::Typed(_)) | Some(TargetFilter::Any)
        ),
        "\"regenerate target creature\" still declares its target"
    );
}

#[test]
fn every_werewolf_survives_the_sweeper_and_the_bear_does_not() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);

    let rise = scenario
        .add_enchantment_from_oracle(P0, "Full Moon's Rise", FULL_MOONS_RISE)
        .id();
    let wolf_a = scenario
        .add_creature(P0, "Mayor of Avabruck", 1, 1)
        .with_subtypes(vec!["Werewolf"])
        .id();
    let wolf_b = scenario
        .add_creature(P0, "Kessig Naturalist", 2, 1)
        .with_subtypes(vec!["Werewolf"])
        .id();
    let bear = scenario.add_creature(P0, "Grizzly Bears", 2, 2).id();
    let sweeper = scenario
        .add_spell_to_hand_from_oracle(P0, "Day of Judgment", false, SWEEPER)
        .id();

    let mut runner = scenario.build();
    runner.activate(rise, 0).resolve();
    let outcome = runner.cast(sweeper).resolve();

    assert_eq!(
        outcome.zone_of(wolf_a),
        Zone::Battlefield,
        "the first Werewolf regenerates — it was shielded by the mass effect"
    );
    assert_eq!(
        outcome.zone_of(wolf_b),
        Zone::Battlefield,
        "the SECOND Werewolf regenerates too: a single-target reading would have \
         shielded only one of them"
    );
    assert_eq!(
        outcome.zone_of(bear),
        Zone::Graveyard,
        "a creature outside the filter is not shielded"
    );
}
