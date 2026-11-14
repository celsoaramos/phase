//! Relatos da mesa (12–13/11/2026): cartas de dado que o motor não jogava.
//!
//! * Endeavor cycle — "Roll two dN and choose one result. <A> equal to that
//!   result. <B> equal to the other result." (CR 706.2 + CR 608.2d): the two
//!   dice are rolled, the controller picks one of the RESULTS (a
//!   `ChoiceType::DieResult` named choice whose options are the rolled values),
//!   "that result" reads the pick and "the other result" the total minus it.
//! * Swarming Goblins — "Create two/three of those tokens" rows of a d20 table
//!   clone the token of the earlier row (CR 706.3a + CR 111.1).
//! * Six-Sided Die — the card is named after its die; the table must still
//!   attach to "Roll a six-sided die." (CR 706.3b).
//! * Velukan Dragon — "+X/+0, where X is the result minus 1".
//!
//! Die faces are forced by seeding the game RNG after the spell is committed,
//! the same mechanism as `comet_stellar_pup.rs`; every forced test asserts the
//! emitted `DieRolled` values as its reach-guard.

use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::types::events::GameEvent;
use engine::types::game_state::{GameRng, RandomDraw, WaitingFor};
use engine::types::identifiers::ObjectId;
use engine::types::phase::Phase;
use engine::types::player::PlayerId;
use engine::types::zones::Zone;
use rand::Rng;

const WILD_ENDEAVOR: &str = "Roll two d4 and choose one result. Create a number of 3/3 green Beast creature tokens equal to that result. Then search your library for a number of basic land cards equal to the other result, put them onto the battlefield tapped, then shuffle.";
const RECKLESS_ENDEAVOR: &str = "Roll two d12 and choose one result. Reckless Endeavor deals damage equal to that result to each creature. Then create a number of Treasure tokens equal to the other result.";
const VALIANT_ENDEAVOR: &str = "Roll two d6 and choose one result. Destroy each creature with power greater than or equal to that result. Then create a number of 2/2 white Knight creature tokens with vigilance equal to the other result.";
const GRAVE_ENDEAVOR: &str = "Roll two d10 and choose one result. Return a creature card from your graveyard to the battlefield with a number of +1/+1 counters on it equal to that result. Then each opponent loses X life and you gain X life, where X is the other result.";
const SWARMING_GOBLINS: &str = "When this creature enters, roll a d20.\n1\u{2014}9 | Create a 1/1 red Goblin creature token.\n10\u{2014}19 | Create two of those tokens.\n20 | Create three of those tokens.";
const SIX_SIDED_DIE: &str = "Choose target creature. Roll a six-sided die.\n1 \u{2014} It has base toughness 1 until end of turn.\n2 \u{2014} Put two -1/-1 counters on it.\n3 \u{2014} Six-Sided Die deals 3 damage to it and you gain 3 life.\n4 \u{2014} It gets -4/-4 until end of turn.\n5 \u{2014} Destroy it.\n6 \u{2014} Exile it.";

/// A seed whose consecutive outcome draws are `faces` on `sides`-sided dice —
/// the same generator and draw `roll_natural` uses.
fn seed_for_faces(sides: u8, faces: &[u8]) -> u64 {
    (0u64..200_000)
        .find(|&seed| {
            let mut rng = GameRng::seed_from_u64(seed);
            faces
                .iter()
                .all(|&face| rng.draw(RandomDraw::Outcome).random_range(1..=sides) == face)
        })
        .expect("a seed exists for the wanted faces")
}

fn force_faces(state: &mut engine::types::game_state::GameState, sides: u8, faces: &[u8]) {
    let seed = seed_for_faces(sides, faces);
    state.rng_seed = seed;
    state.rng_word_pos = 0;
    state.rng = GameRng::seed_from_u64(seed);
}

fn rolled(events: &[GameEvent], sides: u8) -> Vec<u8> {
    events
        .iter()
        .filter_map(|event| match event {
            GameEvent::DieRolled {
                sides: s,
                result: Some(r),
                ..
            } if *s == sides => Some(*r),
            _ => None,
        })
        .collect()
}

fn named_on_battlefield(
    state: &engine::types::game_state::GameState,
    player: PlayerId,
    name: &str,
) -> usize {
    state
        .objects
        .values()
        .filter(|o| o.zone == Zone::Battlefield && o.controller == player && o.name == name)
        .count()
}

fn scenario() -> GameScenario {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario
}

fn add_basic_forests_to_library(scenario: &mut GameScenario, n: usize) -> Vec<ObjectId> {
    (0..n)
        .map(|_| {
            let id = scenario.add_land_to_library_top(P0, "Forest").id();
            id
        })
        .collect()
}

fn make_basic(runner: &mut GameRunner, ids: &[ObjectId]) {
    for id in ids {
        let obj = runner.state_mut().objects.get_mut(id).unwrap();
        obj.card_types
            .supertypes
            .push(engine::types::card_type::Supertype::Basic);
        obj.base_card_types = obj.card_types.clone();
    }
}

#[test]
fn wild_endeavor_offers_the_two_results_and_uses_each_once() {
    let mut scenario = scenario();
    let lands = add_basic_forests_to_library(&mut scenario, 4);
    let spell = scenario
        .add_spell_to_hand_from_oracle(P0, "Wild Endeavor", false, WILD_ENDEAVOR)
        .id();
    let mut runner = scenario.build();
    make_basic(&mut runner, &lands);

    let mut committed = runner
        .cast(spell)
        .choose_option("3")
        .search_first_legal()
        .commit();
    force_faces(committed.state_mut(), 4, &[3, 1]);
    let outcome = committed.resolve();

    assert_eq!(
        rolled(outcome.events(), 4),
        vec![3, 1],
        "reach-guard: forced faces"
    );

    // "that result" = 3 Beasts; "the other result" = 1 basic land.
    assert_eq!(named_on_battlefield(outcome.state(), P0, "Beast"), 3);
    assert_eq!(named_on_battlefield(outcome.state(), P0, "Forest"), 1);
}

#[test]
fn wild_endeavor_choosing_the_low_die_swaps_the_two_effects() {
    let mut scenario = scenario();
    let lands = add_basic_forests_to_library(&mut scenario, 4);
    let spell = scenario
        .add_spell_to_hand_from_oracle(P0, "Wild Endeavor", false, WILD_ENDEAVOR)
        .id();
    let mut runner = scenario.build();
    make_basic(&mut runner, &lands);

    let mut committed = runner
        .cast(spell)
        .choose_option("1")
        .search_first_legal()
        .commit();
    force_faces(committed.state_mut(), 4, &[3, 1]);
    let outcome = committed.resolve();

    assert_eq!(rolled(outcome.events(), 4), vec![3, 1]);
    assert_eq!(named_on_battlefield(outcome.state(), P0, "Beast"), 1);
    assert_eq!(named_on_battlefield(outcome.state(), P0, "Forest"), 3);
}

#[test]
fn the_choice_offers_only_the_rolled_values() {
    let mut scenario = scenario();
    let spell = scenario
        .add_spell_to_hand_from_oracle(P0, "Reckless Endeavor", false, RECKLESS_ENDEAVOR)
        .id();
    let mut runner = scenario.build();
    let mut committed = runner.cast(spell).commit();
    force_faces(committed.state_mut(), 12, &[9, 4]);
    // Pass until the named choice shows up.
    for _ in 0..10 {
        if matches!(
            committed.state().waiting_for,
            WaitingFor::NamedChoice { .. }
        ) {
            break;
        }
        if committed
            .act(engine::types::actions::GameAction::PassPriority)
            .is_err()
        {
            break;
        }
    }
    let WaitingFor::NamedChoice { options, .. } = committed.state().waiting_for.clone() else {
        panic!(
            "expected the die-result choice, got {:?}",
            committed.state().waiting_for
        );
    };
    assert_eq!(options, vec!["9".to_string(), "4".to_string()]);
}

#[test]
fn reckless_endeavor_damage_is_the_chosen_result_and_treasures_the_other() {
    let mut scenario = scenario();
    let small = scenario.add_creature(P1, "Small", 2, 2).id();
    let big = scenario.add_creature(P1, "Big", 6, 6).id();
    let spell = scenario
        .add_spell_to_hand_from_oracle(P0, "Reckless Endeavor", false, RECKLESS_ENDEAVOR)
        .id();
    let mut runner = scenario.build();
    let mut committed = runner.cast(spell).choose_option("4").commit();
    force_faces(committed.state_mut(), 12, &[9, 4]);
    let outcome = committed.resolve();

    assert_eq!(rolled(outcome.events(), 12), vec![9, 4]);
    assert_eq!(
        outcome.zone_of(small),
        Zone::Graveyard,
        "4 damage kills the 2/2"
    );
    assert_eq!(
        outcome.zone_of(big),
        Zone::Battlefield,
        "4 damage does not kill the 6/6"
    );
    assert_eq!(named_on_battlefield(outcome.state(), P0, "Treasure"), 9);
}

#[test]
fn valiant_endeavor_destroys_by_the_chosen_result() {
    let mut scenario = scenario();
    let three = scenario.add_creature(P1, "Three", 3, 3).id();
    let five = scenario.add_creature(P1, "Five", 5, 5).id();
    let spell = scenario
        .add_spell_to_hand_from_oracle(P0, "Valiant Endeavor", false, VALIANT_ENDEAVOR)
        .id();
    let mut runner = scenario.build();
    let mut committed = runner.cast(spell).choose_option("4").commit();
    force_faces(committed.state_mut(), 6, &[4, 2]);
    let outcome = committed.resolve();

    assert_eq!(rolled(outcome.events(), 6), vec![4, 2]);
    assert_eq!(
        outcome.zone_of(three),
        Zone::Battlefield,
        "power 3 < 4 survives"
    );
    assert_eq!(
        outcome.zone_of(five),
        Zone::Graveyard,
        "power 5 >= 4 is destroyed"
    );
    assert_eq!(named_on_battlefield(outcome.state(), P0, "Knight"), 2);
}

#[test]
fn grave_endeavor_counters_and_drain_split_the_two_results() {
    let mut scenario = scenario();
    let corpse = scenario.add_creature(P0, "Corpse", 2, 2).id();
    let spell = scenario
        .add_spell_to_hand_from_oracle(P0, "Grave Endeavor", false, GRAVE_ENDEAVOR)
        .id();
    let mut runner = scenario.build();
    {
        let state = runner.state_mut();
        engine::game::zones::move_to_zone(state, corpse, Zone::Graveyard, &mut Vec::new());
    }
    let mut committed = runner.cast(spell).choose_option("7").commit();
    force_faces(committed.state_mut(), 10, &[7, 3]);
    let outcome = committed.resolve();

    assert_eq!(rolled(outcome.events(), 10), vec![7, 3]);
    let returned = outcome
        .find_object(|o| o.name == "Corpse" && o.zone == Zone::Battlefield)
        .expect("the creature card returns");
    outcome.assert_counters(returned, engine::types::counter::CounterType::Plus1Plus1, 7);
    outcome.assert_life_delta(P1, -3);
    outcome.assert_life_delta(P0, 3);
}

#[test]
fn swarming_goblins_middle_row_creates_two_goblins() {
    let mut scenario = scenario();
    let goblins = scenario
        .add_creature_to_hand_from_oracle(P0, "Swarming Goblins", 1, 1, SWARMING_GOBLINS)
        .id();
    let mut runner = scenario.build();
    let mut committed = runner.cast(goblins).commit();
    force_faces(committed.state_mut(), 20, &[12]);
    let outcome = committed.resolve();
    assert_eq!(rolled(outcome.events(), 20), vec![12]);
    assert_eq!(named_on_battlefield(outcome.state(), P0, "Goblin"), 2);
}

#[test]
fn swarming_goblins_twenty_creates_three_goblins() {
    let mut scenario = scenario();
    let goblins = scenario
        .add_creature_to_hand_from_oracle(P0, "Swarming Goblins", 1, 1, SWARMING_GOBLINS)
        .id();
    let mut runner = scenario.build();
    let mut committed = runner.cast(goblins).commit();
    force_faces(committed.state_mut(), 20, &[20]);
    let outcome = committed.resolve();
    assert_eq!(rolled(outcome.events(), 20), vec![20]);
    assert_eq!(named_on_battlefield(outcome.state(), P0, "Goblin"), 3);
}

#[test]
fn six_sided_die_keeps_its_table() {
    let mut scenario = scenario();
    let victim = scenario.add_creature(P1, "Victim", 4, 4).id();
    let spell = scenario
        .add_spell_to_hand_from_oracle(P0, "Six-Sided Die", true, SIX_SIDED_DIE)
        .id();
    let mut runner = scenario.build();
    let mut committed = runner.cast(spell).target_object(victim).commit();
    force_faces(committed.state_mut(), 6, &[5]);
    let outcome = committed.resolve();
    assert_eq!(rolled(outcome.events(), 6), vec![5]);
    assert_eq!(outcome.zone_of(victim), Zone::Graveyard, "5 — Destroy it.");
}

const DELINA: &str = "Whenever Delina attacks, choose target creature you control, then roll a d20.\n1\u{2014}14 | Create a tapped and attacking token that's a copy of that creature, except it's not legendary and it has \"At end of combat, exile this token.\"\n15\u{2014}20 | Create one of those tokens. You may roll again.";
const VELUKAN: &str = "Whenever this creature attacks or blocks, roll a six-sided die. This creature gets +X/+0 until end of turn, where X is the result minus 1.";

/// Drive Delina's attack trigger: target the Bear, then answer every "you may
/// roll again" with `accept`. Returns the copies of the Bear on the battlefield.
fn delina_attack(faces: &[u8], accept: bool) -> (usize, Vec<u8>) {
    use engine::game::combat::AttackTarget;
    use engine::types::ability::TargetRef;
    use engine::types::actions::GameAction;

    let mut scenario = scenario();
    let delina = scenario
        .add_creature_from_oracle(P0, "Delina, Wild Mage", 3, 2, DELINA)
        .id();
    let bear = scenario.add_creature(P0, "Bear", 2, 2).id();
    let mut runner = scenario.build();
    runner.advance_to_combat();
    runner
        .declare_attackers(&[(delina, AttackTarget::Player(P1))])
        .expect("Delina attacks");
    let mut events: Vec<GameEvent> = Vec::new();
    let mut forced = false;
    for _ in 0..60 {
        let waiting = runner.state().waiting_for.clone();
        let result = match waiting {
            WaitingFor::TriggerTargetSelection { .. } | WaitingFor::TargetSelection { .. } => {
                runner.act(GameAction::ChooseTarget {
                    target: Some(TargetRef::Object(bear)),
                })
            }
            WaitingFor::OptionalEffectChoice { .. } => {
                runner.act(GameAction::DecideOptionalEffect { accept })
            }
            WaitingFor::Priority { .. } => {
                if runner.state().stack.is_empty() {
                    break;
                }
                if !forced {
                    force_faces(runner.state_mut(), 20, faces);
                    forced = true;
                }
                runner.act(GameAction::PassPriority)
            }
            other => panic!("unexpected prompt {other:?}"),
        };
        let r = result.expect("action accepted");
        events.extend(r.events);
    }
    let copies = runner
        .state()
        .objects
        .values()
        .filter(|o| o.zone == Zone::Battlefield && o.name == "Bear" && o.is_token)
        .count();
    (copies, rolled(&events, 20))
}

#[test]
fn delina_low_roll_makes_one_copy_and_stops() {
    let (copies, rolls) = delina_attack(&[5], true);
    assert_eq!(rolls, vec![5]);
    assert_eq!(copies, 1);
}

#[test]
fn delina_high_roll_makes_a_copy_and_rolls_again_when_accepted() {
    // 17 (15–20: copy + may roll again) → accept → 5 (1–14: copy, stop).
    let (copies, rolls) = delina_attack(&[17, 5], true);
    assert_eq!(rolls, vec![17, 5]);
    assert_eq!(copies, 2);
}

#[test]
fn delina_high_roll_stops_when_declined() {
    let (copies, rolls) = delina_attack(&[17, 5], false);
    assert_eq!(rolls, vec![17]);
    assert_eq!(copies, 1);
}

#[test]
fn velukan_dragon_gets_the_result_minus_one() {
    use engine::game::combat::AttackTarget;
    use engine::types::actions::GameAction;

    let mut scenario = scenario();
    let dragon = scenario
        .add_creature_from_oracle(P0, "Velukan Dragon", 4, 4, VELUKAN)
        .id();
    let mut runner = scenario.build();
    runner.advance_to_combat();
    runner
        .declare_attackers(&[(dragon, AttackTarget::Player(P1))])
        .expect("the dragon attacks");
    force_faces(runner.state_mut(), 6, &[4]);
    let mut events = Vec::new();
    for _ in 0..10 {
        if runner.state().stack.is_empty() {
            break;
        }
        events.extend(runner.act(GameAction::PassPriority).expect("pass").events);
    }
    assert_eq!(rolled(&events, 6), vec![4]);
    let obj = &runner.state().objects[&dragon];
    assert_eq!(
        (obj.power, obj.toughness),
        (Some(7), Some(4)),
        "4 + (4 - 1) = 7"
    );
}
