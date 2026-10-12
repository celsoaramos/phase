//! CR 700.12: "non-outlaw creature" excludes Assassins, Mercenaries, Pirates,
//! Rogues, and Warlocks. The negation used to parse as `Non(Subtype("Outlaw"))`
//! — a subtype no object has — so Shoot the Sheriff destroyed a Human Pirate
//! (Angrath's Marauders) in a real game.

use engine::game::scenario::{GameScenario, P0, P1};
use engine::types::phase::Phase;
use engine::types::zones::Zone;

const SHOOT_THE_SHERIFF: &str = "Destroy target non-outlaw creature. (Assassins, Mercenaries, \
Pirates, Rogues, and Warlocks are outlaws. Everyone else is fair game.)";

#[test]
fn shoot_the_sheriff_cannot_target_a_pirate() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let pirate = scenario
        .add_creature(P1, "Angrath's Marauders", 6, 6)
        .with_subtypes(vec!["Human", "Pirate"])
        .id();
    let spell = scenario
        .add_spell_to_hand_from_oracle(P0, "Shoot the Sheriff", true, SHOOT_THE_SHERIFF)
        .id();

    let mut runner = scenario.build();
    let result = runner.cast(spell).target_object(pirate).try_resolve();
    assert!(
        result.is_err(),
        "a Pirate is an outlaw and must not be a legal target"
    );
    assert_eq!(
        runner.state().objects.get(&pirate).map(|o| o.zone),
        Some(Zone::Battlefield)
    );
}

#[test]
fn shoot_the_sheriff_destroys_a_non_outlaw_creature() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let human = scenario
        .add_creature(P1, "Kefka, Court Mage", 4, 5)
        .with_subtypes(vec!["Human", "Wizard"])
        .id();
    let spell = scenario
        .add_spell_to_hand_from_oracle(P0, "Shoot the Sheriff", true, SHOOT_THE_SHERIFF)
        .id();

    let mut runner = scenario.build();
    let outcome = runner.cast(spell).target_object(human).resolve();
    assert_eq!(
        outcome.state().objects.get(&human).map(|o| o.zone),
        Some(Zone::Graveyard)
    );
}
