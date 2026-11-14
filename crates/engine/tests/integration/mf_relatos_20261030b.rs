//! Field reports of 2026-10-30 (MagicFinder mesa, engine v0.118.0):
//!
//! * Surgical Suite // Hospital Room — the right door was unlocked and its
//!   "Whenever you attack" trigger never fired. The door text was lost when the
//!   game was restored (undo / reload): rehydration re-stamped only the cast
//!   half's printed triggers on the battlefield Room. Secret Arcade // Dusty
//!   Parlor ("Whenever you cast an enchantment spell", relato de 30/10 on
//!   v0.119.0) is the same defect.
//! * Glimmer Seeker — tapped from attacking, no Glimmer creature, and the
//!   survival trigger created no token in the second main phase.
//! * Sword of Hearth and Home — the exiled creature never came back.

use engine::game::effects::attach::attach_to;
use engine::game::game_object::RoomDoor;
use engine::game::layers::evaluate_layers;
use engine::game::scenario::{GameRunner, GameScenario, P0};
use engine::game::scenario_db::GameScenarioDbExt;
use engine::types::ability::TargetRef;
use engine::types::actions::GameAction;
use engine::types::card_type::Supertype;
use engine::types::game_state::WaitingFor;
use engine::types::mana::{ManaType, ManaUnit};
use engine::types::phase::Phase;
use engine::types::triggers::TriggerMode;
use engine::types::zones::Zone;

use super::rules::run_combat;
use crate::support::shared_card_db;

const GLIMMER_SEEKER: &str = "Survival — At the beginning of your second main phase, if this creature is tapped, draw a card if you control a Glimmer creature. If you don't control a Glimmer creature, create a 1/1 white Glimmer enchantment creature token.";

const SWORD_OF_HEARTH_AND_HOME_TRIGGER: &str = "Whenever equipped creature deals combat damage to a player, exile up to one target creature you own, then search your library for a basic land card. Put both cards onto the battlefield under your control, then shuffle.";

fn drive_to_empty_stack(runner: &mut GameRunner, target: Option<TargetRef>) {
    for _ in 0..60 {
        match runner.state().waiting_for.clone() {
            WaitingFor::TriggerTargetSelection { target_slots, .. } => {
                let chosen = target
                    .clone()
                    .filter(|t| target_slots[0].legal_targets.contains(t))
                    .or_else(|| target_slots[0].legal_targets.first().cloned());
                runner
                    .act(GameAction::SelectTargets {
                        targets: chosen.into_iter().collect(),
                    })
                    .expect("target selection must succeed");
            }
            WaitingFor::SearchChoice { cards, count, .. } => {
                runner
                    .act(GameAction::SelectCards {
                        cards: cards.into_iter().take(count).collect(),
                    })
                    .expect("search selection must succeed");
            }
            WaitingFor::OptionalEffectChoice { .. } => {
                runner
                    .act(GameAction::DecideOptionalEffect { accept: true })
                    .expect("optional must accept");
            }
            WaitingFor::Priority { .. } => {
                if runner.state().stack.is_empty() {
                    return;
                }
                runner
                    .act(GameAction::PassPriority)
                    .expect("passing priority resolves the stack");
            }
            other => panic!("unexpected wait while resolving: {other:?}"),
        }
    }
    panic!("stack never emptied");
}

fn sword_runner(with_forest: bool) -> (GameRunner, engine::types::identifiers::ObjectId) {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let bearer = scenario.add_creature(P0, "Sword Bearer", 2, 2).id();
    let sword = scenario
        .add_creature(P0, "Sword of Hearth and Home", 0, 0)
        .as_artifact()
        .with_subtypes(vec!["Equipment"])
        .from_oracle_text(SWORD_OF_HEARTH_AND_HOME_TRIGGER)
        .id();
    let colossus = scenario.add_creature(P0, "Exiled Colossus", 5, 5).id();
    let forest = with_forest.then(|| {
        let mut builder = scenario.add_land_to_library_top(P0, "Forest");
        builder.with_subtypes(vec!["Forest"]);
        builder.id()
    });
    let mut runner = scenario.build();
    if let Some(forest) = forest {
        let obj = runner.state_mut().objects.get_mut(&forest).unwrap();
        obj.card_types.supertypes.push(Supertype::Basic);
        obj.base_card_types = obj.card_types.clone();
    }
    attach_to(runner.state_mut(), sword, bearer);
    evaluate_layers(runner.state_mut());
    run_combat(&mut runner, vec![bearer], vec![]);
    drive_to_empty_stack(&mut runner, Some(TargetRef::Object(colossus)));
    (runner, colossus)
}

/// CR 701.23a + CR 608.2c: "Put both cards onto the battlefield"
/// returns the exiled creature together with the found basic land.
#[test]
fn sword_of_hearth_and_home_returns_exiled_creature_with_land() {
    let (runner, colossus) = sword_runner(true);
    assert_eq!(
        runner.state().objects[&colossus].zone,
        Zone::Battlefield,
        "the exiled creature must come back with the land"
    );
    let forests = runner
        .state()
        .objects
        .values()
        .filter(|o| o.name == "Forest" && o.zone == Zone::Battlefield)
        .count();
    assert_eq!(forests, 1, "the searched basic land enters too");
}

/// CR 701.23d: failing to find a land doesn't stop the exiled creature from
/// returning — "both cards" is whatever cards the instructions produced.
#[test]
fn sword_of_hearth_and_home_returns_creature_when_search_finds_nothing() {
    let (runner, colossus) = sword_runner(false);
    assert_eq!(
        runner.state().objects[&colossus].zone,
        Zone::Battlefield,
        "the exiled creature must come back even when no land was found"
    );
}

/// CR 207.2c (survival is an ability word) + CR 608.2c: the tapped Glimmer
/// Seeker with no Glimmer creature creates a Glimmer token in the second main.
#[test]
fn glimmer_seeker_creates_token_without_glimmer() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let seeker = scenario
        .add_creature_from_oracle(P0, "Glimmer Seeker", 3, 3, GLIMMER_SEEKER)
        .id();
    let mut runner = scenario.build();
    run_combat(&mut runner, vec![seeker], vec![]);
    runner.advance_to_phase(Phase::PostCombatMain);
    drive_to_empty_stack(&mut runner, None);
    let glimmers = runner
        .state()
        .objects
        .values()
        .filter(|o| o.zone == Zone::Battlefield && o.name == "Glimmer")
        .count();
    assert_eq!(glimmers, 1, "survival must create a Glimmer token");
}

fn white_mana(source: engine::types::identifiers::ObjectId, n: usize) -> Vec<ManaUnit> {
    (0..n)
        .map(|_| ManaUnit::new(ManaType::White, source, false, vec![]))
        .collect()
}

/// CR 709.5e + CR 709.5: a door unlocked this turn has its rules text right
/// away — the attack trigger of the newly unlocked door fires the same turn.
#[test]
fn room_door_unlocked_this_turn_has_its_attack_trigger() {
    let db = shared_card_db().expect("fixture loads");
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let attacker = scenario.add_creature(P0, "Attacker", 2, 2).id();
    let room = scenario.add_real_card(P0, "Dollmaker's Shop", Zone::Hand, db);
    scenario.with_mana_pool(P0, white_mana(room, 8));
    let mut runner = scenario.build();
    engine::game::rehydrate_game_from_card_db(runner.state_mut(), db);
    // Cast the RIGHT half (Porcelain Gallery); Dollmaker's Shop stays locked.
    runner.cast(room).modal_back_face(true).resolve();
    runner
        .act(GameAction::UnlockRoomDoor {
            object_id: room,
            door: RoomDoor::Left,
        })
        .expect("unlocking the locked door");
    drive_to_empty_stack(&mut runner, None);
    assert!(runner.state().objects[&room]
        .room_unlocks
        .unwrap()
        .is_unlocked(RoomDoor::Left));
    run_combat(&mut runner, vec![attacker], vec![]);
    drive_to_empty_stack(&mut runner, None);
    let toys = runner
        .state()
        .objects
        .values()
        .filter(|o| o.zone == Zone::Battlefield && o.name == "Toy")
        .count();
    assert_eq!(toys, 1, "Dollmaker's Shop unlocked this turn must trigger");
}

/// Same as above with the LEFT half cast and the RIGHT door unlocked this turn
/// (the Surgical Suite // Hospital Room orientation).
#[test]
fn room_right_door_unlocked_this_turn_has_its_combat_trigger() {
    let db = shared_card_db().expect("fixture loads");
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let attacker = scenario.add_creature(P0, "Attacker", 2, 2).id();
    let room = scenario.add_real_card(P0, "Bottomless Pool", Zone::Hand, db);
    scenario.with_mana_pool(
        P0,
        (0..6)
            .map(|_| ManaUnit::new(ManaType::Blue, room, false, vec![]))
            .collect(),
    );
    scenario.with_library_top(P0, &["Card A", "Card B", "Card C"]);
    let mut runner = scenario.build();
    engine::game::rehydrate_game_from_card_db(runner.state_mut(), db);
    runner.cast(room).modal_back_face(false).resolve();
    drive_to_empty_stack(&mut runner, None);
    runner
        .act(GameAction::UnlockRoomDoor {
            object_id: room,
            door: RoomDoor::Right,
        })
        .expect("unlocking Locker Room");
    drive_to_empty_stack(&mut runner, None);
    let hand_before = runner.state().players[0].hand.len();
    run_combat(&mut runner, vec![attacker], vec![]);
    drive_to_empty_stack(&mut runner, None);
    assert_eq!(
        runner.state().players[0].hand.len(),
        hand_before + 1,
        "Locker Room unlocked this turn must draw on combat damage"
    );
}

/// The other branch: with a Glimmer creature the tapped Seeker draws and makes
/// no token — the second sentence is evaluated on its own guard, never both.
#[test]
fn glimmer_seeker_draws_with_glimmer_and_makes_no_token() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let seeker = scenario
        .add_creature_from_oracle(P0, "Glimmer Seeker", 3, 3, GLIMMER_SEEKER)
        .id();
    scenario
        .add_creature(P0, "Inquisitive Glimmer", 2, 3)
        .with_subtypes(vec!["Glimmer"]);
    scenario.with_library_top(P0, &["Card A", "Card B"]);
    let mut runner = scenario.build();
    let hand_before = runner.state().players[0].hand.len();
    run_combat(&mut runner, vec![seeker], vec![]);
    runner.advance_to_phase(Phase::PostCombatMain);
    drive_to_empty_stack(&mut runner, None);
    let tokens = runner
        .state()
        .objects
        .values()
        .filter(|o| o.zone == Zone::Battlefield && o.name == "Glimmer")
        .count();
    assert_eq!(tokens, 0, "with a Glimmer creature, no token");
    assert_eq!(
        runner.state().players[0].hand.len(),
        hand_before + 1,
        "with a Glimmer creature, the Seeker draws"
    );
}

/// The mesa's save/undo restore pipeline, rehydrating from the card database
/// exactly like the WASM `restoreState`.
fn round_trip_rehydrated(
    runner: &GameRunner,
    db: &'static engine::database::CardDatabase,
) -> GameRunner {
    use engine::types::game_state::{PersistedGameState, PersistedRestoreFinalization};
    let json = serde_json::to_string(&PersistedGameState::capture(runner.state().clone()))
        .expect("state serializes");
    let state = serde_json::from_str::<PersistedGameState>(&json)
        .expect("persisted state decodes")
        .prepare_for_restore(PersistedRestoreFinalization::DeferUntilRehydrated)
        .expect("persisted state is admissible")
        .finalize_after_rehydration(|state| {
            engine::game::rehydrate_game_from_card_db(state, db);
            Ok(())
        })
        .expect("restored state is publishable");
    GameRunner::from_state(state)
}

/// CR 709.5: a restored game (undo, reload) keeps the OTHER door's rules text:
/// unlocking it after the restore still gives the Room that door's trigger.
#[test]
fn room_right_door_trigger_survives_save_restore() {
    let db = shared_card_db().expect("fixture loads");
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let room = scenario.add_real_card(P0, "Bottomless Pool", Zone::Hand, db);
    scenario.with_mana_pool(
        P0,
        (0..6)
            .map(|_| ManaUnit::new(ManaType::Blue, room, false, vec![]))
            .collect(),
    );
    scenario.with_library_top(P0, &["Card A", "Card B", "Card C"]);
    let mut runner = scenario.build();
    engine::game::rehydrate_game_from_card_db(runner.state_mut(), db);
    runner.cast(room).modal_back_face(false).resolve();
    drive_to_empty_stack(&mut runner, None);
    let mut runner = round_trip_rehydrated(&runner, db);
    runner
        .act(GameAction::UnlockRoomDoor {
            object_id: room,
            door: RoomDoor::Right,
        })
        .expect("unlocking Locker Room after a restore");
    drive_to_empty_stack(&mut runner, None);
    let obj = &runner.state().objects[&room];
    assert!(
        obj.base_trigger_definitions.iter().any(|definition| {
            definition.mode == TriggerMode::DamageDoneOnceByController
                && definition.room_door == Some(RoomDoor::Right)
        }),
        "Locker Room's door text must still be on the Room after a restore"
    );
}

/// Secret Arcade // Dusty Parlor (relato de 30/10): the right door's "Whenever
/// you cast an enchantment spell" with both doors unlocked. `restore` runs the
/// game through the save/undo pipeline after the unlock, as the mesa does.
fn dusty_parlor_counters_after_enchantment_cast(restore: bool) -> u32 {
    let db = shared_card_db().expect("fixture loads");
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let creature = scenario.add_creature(P0, "Counter Target", 2, 2).id();
    let room = scenario.add_real_card(P0, "Secret Arcade", Zone::Hand, db);
    let enchantment = scenario
        .add_spell_to_hand_from_oracle(P0, "Test Enchantment", false, "")
        .as_enchantment()
        .with_mana_cost(engine::types::mana::ManaCost::Cost {
            shards: vec![engine::types::mana::ManaCostShard::White],
            generic: 1,
        })
        .id();
    scenario.with_mana_pool(P0, white_mana(room, 10));
    let mut runner = scenario.build();
    engine::game::rehydrate_game_from_card_db(runner.state_mut(), db);
    runner.cast(room).modal_back_face(false).resolve();
    drive_to_empty_stack(&mut runner, None);
    runner
        .act(GameAction::UnlockRoomDoor {
            object_id: room,
            door: RoomDoor::Right,
        })
        .expect("unlocking Dusty Parlor");
    drive_to_empty_stack(&mut runner, None);
    let mut runner = if restore {
        round_trip_rehydrated(&runner, db)
    } else {
        runner
    };
    runner
        .act(GameAction::CastSpell {
            object_id: enchantment,
            card_id: runner.state().objects[&enchantment].card_id,
            targets: Vec::new(),
            payment_mode: engine::types::game_state::CastPaymentMode::Auto,
        })
        .expect("casting the enchantment");
    drive_to_empty_stack(&mut runner, Some(TargetRef::Object(creature)));
    runner.state().objects[&creature]
        .counters
        .get(&engine::types::counter::CounterType::Plus1Plus1)
        .copied()
        .unwrap_or(0)
}

/// CR 709.5: Dusty Parlor puts counters equal to the enchantment's mana value.
#[test]
fn dusty_parlor_triggers_on_enchantment_cast() {
    assert_eq!(dusty_parlor_counters_after_enchantment_cast(false), 2);
}

/// CR 709.5: …and keeps doing so after the mesa restores the game.
#[test]
fn dusty_parlor_triggers_after_save_restore() {
    assert_eq!(dusty_parlor_counters_after_enchantment_cast(true), 2);
}
