//! "That Vehicle" / "that Mount" on an actor-side crews/saddles trigger is the
//! crewed Vehicle / saddled Mount (CR 702.122, CR 702.171).
//!
//! MagicFinder report (2026-10-18): Vehicles and Mounts crewed or saddled by
//! Canyon Vaulter never gained flying. The effect's subject parses as
//! `TriggeringSource`, and `extract_source_from_event` had no arm for
//! `VehicleCrewed` / `Saddled`, so "that Mount or Vehicle" named no object and
//! the grant applied to nothing. Gearshift Ace's "that Vehicle" had the same hole.

use engine::game::scenario::{GameRunner, GameScenario, P0};
use engine::game::zones::create_object;
use engine::types::actions::GameAction;
use engine::types::card_type::CoreType;
use engine::types::identifiers::{CardId, ObjectId};
use engine::types::keywords::Keyword;
use engine::types::phase::Phase;
use engine::types::zones::Zone;

const CANYON_VAULTER: &str = "Whenever this creature saddles a Mount or crews a Vehicle during your main phase, that Mount or Vehicle gains flying until end of turn.";
const GEARSHIFT_ACE: &str =
    "Whenever this creature crews a Vehicle, that Vehicle gains flying until end of turn.";

fn has_flying(runner: &GameRunner, id: ObjectId) -> bool {
    runner.state().objects[&id].has_keyword(&Keyword::Flying)
}

fn add_vehicle(runner: &mut GameRunner) -> ObjectId {
    let next_id = runner.state().next_object_id;
    let vehicle = create_object(
        runner.state_mut(),
        CardId(next_id),
        P0,
        "Test Vehicle".to_string(),
        Zone::Battlefield,
    );
    let obj = runner.state_mut().objects.get_mut(&vehicle).unwrap();
    obj.card_types.core_types.push(CoreType::Artifact);
    obj.card_types.subtypes.push("Vehicle".to_string());
    obj.base_card_types = obj.card_types.clone();
    obj.keywords.push(Keyword::Crew {
        power: 2,
        once_per_turn: None,
    });
    obj.power = Some(4);
    obj.toughness = Some(2);
    obj.base_power = Some(4);
    obj.base_toughness = Some(2);
    obj.summoning_sick = false;
    vehicle
}

fn crew(runner: &mut GameRunner, vehicle: ObjectId, crew: ObjectId) {
    runner
        .act(GameAction::CrewVehicle {
            vehicle_id: vehicle,
            creature_ids: vec![],
        })
        .expect("enter crew mode");
    runner
        .act(GameAction::CrewVehicle {
            vehicle_id: vehicle,
            creature_ids: vec![crew],
        })
        .expect("crewing must be legal");
    runner.advance_until_stack_empty();
}

/// CR 702.171a + CR 702.171c: Canyon Vaulter saddles a Mount during its
/// controller's main phase — that Mount gains flying; the Vaulter does not.
#[test]
fn canyon_vaulter_saddle_grants_the_mount_flying() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let vaulter = scenario
        .add_creature_from_oracle(P0, "Canyon Vaulter", 3, 1, CANYON_VAULTER)
        .id();
    let mount = {
        let mut b = scenario.add_creature(P0, "Bulwark Ox", 2, 2);
        b.with_keyword(Keyword::Saddle(1));
        b.id()
    };
    let mut runner = scenario.build();
    runner
        .act(GameAction::SaddleMount {
            mount_id: mount,
            creature_ids: vec![],
        })
        .expect("enter saddle mode");
    runner
        .act(GameAction::SaddleMount {
            mount_id: mount,
            creature_ids: vec![vaulter],
        })
        .expect("saddling must be legal");
    runner.advance_until_stack_empty();

    assert!(
        runner.state().objects[&mount].is_saddled,
        "premise: the Mount is saddled"
    );
    assert!(has_flying(&runner, mount), "that Mount must gain flying");
    assert!(
        !has_flying(&runner, vaulter),
        "the saddling creature itself must not gain flying"
    );
}

/// CR 702.122: Canyon Vaulter crews a Vehicle during the main phase — that
/// Vehicle gains flying.
#[test]
fn canyon_vaulter_crew_grants_the_vehicle_flying() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let vaulter = scenario
        .add_creature_from_oracle(P0, "Canyon Vaulter", 3, 1, CANYON_VAULTER)
        .id();
    let mut runner = scenario.build();
    let vehicle = add_vehicle(&mut runner);
    crew(&mut runner, vehicle, vaulter);

    assert!(
        has_flying(&runner, vehicle),
        "that Vehicle must gain flying"
    );
    assert!(!has_flying(&runner, vaulter));
}

/// CR 505.1: "during your main phase" — crewing at beginning of combat does
/// not trigger, so the Vehicle stays grounded.
#[test]
fn canyon_vaulter_crew_outside_main_phase_grants_nothing() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::BeginCombat);
    let vaulter = scenario
        .add_creature_from_oracle(P0, "Canyon Vaulter", 3, 1, CANYON_VAULTER)
        .id();
    let mut runner = scenario.build();
    let vehicle = add_vehicle(&mut runner);
    crew(&mut runner, vehicle, vaulter);

    assert!(
        !has_flying(&runner, vehicle),
        "crewing outside the main phase must not grant flying"
    );
}

/// CR 702.122: the same anaphor without a phase restriction (Gearshift Ace).
#[test]
fn gearshift_ace_crew_grants_the_vehicle_flying() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::BeginCombat);
    let ace = scenario
        .add_creature_from_oracle(P0, "Gearshift Ace", 2, 2, GEARSHIFT_ACE)
        .id();
    let mut runner = scenario.build();
    let vehicle = add_vehicle(&mut runner);
    crew(&mut runner, vehicle, ace);

    assert!(
        has_flying(&runner, vehicle),
        "that Vehicle must gain flying"
    );
    assert!(!has_flying(&runner, ace));
}
