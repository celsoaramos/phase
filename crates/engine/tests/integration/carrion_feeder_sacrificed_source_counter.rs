//! Carrion Feeder sacrificed to pay its own "Sacrifice a creature: Put a +1/+1
//! counter on this creature" while under Supernatural Stamina (field report,
//! 2026-09-28): the granted dies trigger returns the Feeder before the
//! activated ability resolves, and the returned Feeder got the counter.
//!
//! CR 400.7: an object that moves from one zone to another becomes a new object
//! with no memory of its previous existence. The ability's "this creature"
//! refers to the Feeder that was sacrificed; the Feeder that came back is a
//! different object, so the counter has nowhere to go (CR 608.2b).
//!
//! The counter and tap/untap resolvers short-circuit `SelfRef` straight to
//! `ability.source_id`. The activated ability captured the Feeder's graveyard
//! incarnation (the sacrifice is paid before it reaches the stack), and the
//! returned Feeder has a newer one, so `source_is_current` is already false —
//! but nothing read it. `self_ref_is_current` is not the gate either: it fails
//! open for every non-triggered ability, and gating triggered sources through it
//! broke Forgotten Ancient and Bogardan Phoenix. The fix is
//! `ResolvedAbility::self_ref_left_before_resolution` (non-triggered only);
//! Suspend (exile this card from hand as a cost, then time counters on it) stays
//! current because the incarnation is captured after that cost.

use engine::game::scenario::{GameScenario, P0};
use engine::types::counter::CounterType;
use engine::types::identifiers::ObjectId;
use engine::types::mana::{ManaType, ManaUnit};
use engine::types::phase::Phase;
use engine::types::zones::Zone;

const CARRION_FEEDER_ORACLE: &str =
    "This creature can't block.\nSacrifice a creature: Put a +1/+1 counter on this creature.";

const SUPERNATURAL_STAMINA_ORACLE: &str = "Until end of turn, target creature gets +2/+0 and \
gains \"When this creature dies, return it to the battlefield tapped under its owner's control.\"";

fn feeder_ability_index(state: &engine::types::game_state::GameState, feeder: ObjectId) -> usize {
    state.objects[&feeder]
        .abilities
        .iter()
        .position(|a| a.cost.is_some())
        .expect("Carrion Feeder has a costed activated ability")
}

fn p1p1(state: &engine::types::game_state::GameState, id: ObjectId) -> u32 {
    state.objects[&id]
        .counters
        .get(&CounterType::Plus1Plus1)
        .copied()
        .unwrap_or(0)
}

/// Control: sacrificing ANOTHER creature still grows the Feeder.
#[test]
fn sacrificing_another_creature_puts_the_counter_on_the_feeder() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let feeder = scenario
        .add_creature_from_oracle(P0, "Carrion Feeder", 1, 1, CARRION_FEEDER_ORACLE)
        .id();
    let fodder = scenario.add_creature(P0, "Fodder", 1, 1).id();

    let mut runner = scenario.build();
    let idx = feeder_ability_index(runner.state(), feeder);
    runner.activate(feeder, idx).pay_with(&[fodder]).resolve();

    assert_eq!(runner.state().objects[&fodder].zone, Zone::Graveyard);
    assert_eq!(p1p1(runner.state(), feeder), 1);
}

/// CR 400.7: the Feeder sacrificed for its own ability and returned by
/// Supernatural Stamina before the ability resolves is a new object — it must
/// NOT receive the counter.
#[test]
fn feeder_sacrificed_to_itself_and_returned_gets_no_counter() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.with_mana_pool(
        P0,
        vec![ManaUnit::new(ManaType::Black, ObjectId(0), false, vec![])],
    );
    let feeder = scenario
        .add_creature_from_oracle(P0, "Carrion Feeder", 1, 1, CARRION_FEEDER_ORACLE)
        .id();
    let stamina = scenario
        .add_spell_to_hand_from_oracle(
            P0,
            "Supernatural Stamina",
            true,
            SUPERNATURAL_STAMINA_ORACLE,
        )
        .id();

    let mut runner = scenario.build();
    runner.cast(stamina).target_objects(&[feeder]).resolve();

    let idx = feeder_ability_index(runner.state(), feeder);
    runner.activate(feeder, idx).pay_with(&[feeder]).resolve();
    runner.advance_until_stack_empty();

    assert_eq!(
        runner.state().objects[&feeder].zone,
        Zone::Battlefield,
        "control: Supernatural Stamina's granted dies trigger must return the Feeder"
    );
    assert_eq!(
        p1p1(runner.state(), feeder),
        0,
        "the returned Feeder is a new object (CR 400.7): the ability's counter has no referent"
    );
}

/// Black Carriage (Oracle, minus its upkeep-only restriction so the scenario
/// can run in the main phase): "Sacrifice a creature: Untap this creature."
const BLACK_CARRIAGE_ORACLE: &str = "Sacrifice a creature: Untap this creature.";

/// CR 400.7 + CR 701.26b: the same shape through the tap/untap path. The
/// Carriage sacrificed to its own untap ability comes back TAPPED from
/// Supernatural Stamina; the untap belonged to the old object, so the new one
/// must stay tapped.
#[test]
fn carriage_sacrificed_to_itself_and_returned_stays_tapped() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.with_mana_pool(
        P0,
        vec![ManaUnit::new(ManaType::Black, ObjectId(0), false, vec![])],
    );
    let carriage = scenario
        .add_creature_from_oracle(P0, "Black Carriage", 4, 4, BLACK_CARRIAGE_ORACLE)
        .id();
    let stamina = scenario
        .add_spell_to_hand_from_oracle(
            P0,
            "Supernatural Stamina",
            true,
            SUPERNATURAL_STAMINA_ORACLE,
        )
        .id();

    let mut runner = scenario.build();
    runner.cast(stamina).target_objects(&[carriage]).resolve();

    let idx = feeder_ability_index(runner.state(), carriage);
    runner
        .activate(carriage, idx)
        .pay_with(&[carriage])
        .resolve();
    runner.advance_until_stack_empty();

    assert_eq!(
        runner.state().objects[&carriage].zone,
        Zone::Battlefield,
        "control: Supernatural Stamina's granted dies trigger must return the Carriage"
    );
    assert!(
        runner.state().objects[&carriage].tapped,
        "the returned Carriage entered tapped and is a new object (CR 400.7): the old \
         object's untap must not reach it"
    );
}

/// Control: sacrificing ANOTHER creature still untaps the Carriage.
#[test]
fn sacrificing_another_creature_untaps_the_carriage() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let carriage = scenario
        .add_creature_from_oracle(P0, "Black Carriage", 4, 4, BLACK_CARRIAGE_ORACLE)
        .id();
    let fodder = scenario.add_creature(P0, "Fodder", 1, 1).id();

    let mut runner = scenario.build();
    runner
        .state_mut()
        .objects
        .get_mut(&carriage)
        .unwrap()
        .tapped = true;
    assert!(
        runner.state().objects[&carriage].tapped,
        "precondition: the Carriage starts tapped"
    );
    let idx = feeder_ability_index(runner.state(), carriage);
    runner.activate(carriage, idx).pay_with(&[fodder]).resolve();

    assert!(
        !runner.state().objects[&carriage].tapped,
        "sacrificing another creature untaps the Carriage"
    );
}
