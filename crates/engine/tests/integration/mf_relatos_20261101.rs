//! Relatos de 01/11/2026 (MagicFinder).
//!
//! Slimy Aquarium (Underwater Tunnel // Slimy Aquarium): "When you unlock this
//! door, manifest dread, then put a +1/+1 counter on that creature." The door
//! trigger's subject is the PLAYER ("you"), and the demonstrative re-anchor for
//! a non-self trigger subject bound "that creature" to `TriggeringSource` — the
//! Room — instead of the creature manifest dread just produced (CR 608.2c), so
//! the counter went nowhere. A player can't be the antecedent of "that
//! creature" (CR 109.5): once the chain produced a permanent, it names that.

use engine::game::game_object::{BackFaceData, RoomDoor, RoomUnlockState};
use engine::game::scenario::{GameRunner, GameScenario, P0};
use engine::parser::parse_oracle_text;
use engine::types::ability::{Effect, TargetFilter};
use engine::types::actions::GameAction;
use engine::types::card::LayoutKind;
use engine::types::card_type::{CardType, CoreType};
use engine::types::counter::CounterType;
use engine::types::game_state::WaitingFor;
use engine::types::identifiers::ObjectId;
use engine::types::mana::ManaCost;
use engine::types::phase::Phase;

const SLIMY_AQUARIUM: &str =
    "When you unlock this door, manifest dread, then put a +1/+1 counter on that creature.";
const UNDERWATER_TUNNEL: &str = "When you unlock this door, surveil 2.";

fn door_counter_target(oracle: &str) -> TargetFilter {
    let parsed = parse_oracle_text(
        oracle,
        "Door",
        &[],
        &["Enchantment".to_string()],
        &["Room".to_string()],
    );
    let counter = parsed.triggers[0]
        .execute
        .as_deref()
        .and_then(|execute| execute.sub_ability.as_deref())
        .expect("the counter clause is the producer's continuation");
    match &*counter.effect {
        Effect::PutCounter { target, .. } => target.clone(),
        other => panic!("expected PutCounter, got {other:?}"),
    }
}

/// Parse half: the counter binds the chain-created referent, not the Room.
#[test]
fn slimy_aquarium_counter_binds_the_manifested_creature() {
    assert_eq!(
        door_counter_target(SLIMY_AQUARIUM),
        TargetFilter::LastCreated
    );
}

/// Same player subject, other player-actor forms of the trigger head.
#[test]
fn player_subject_triggers_bind_the_created_token() {
    for oracle in [
        "Whenever you attack, create a 1/1 white Soldier creature token, then put a +1/+1 counter on that creature.",
        "Whenever an opponent attacks you, manifest dread, then put a +1/+1 counter on that creature.",
    ] {
        let parsed = parse_oracle_text(oracle, "Probe", &[], &["Enchantment".to_string()], &[]);
        let counter = parsed.triggers[0]
            .execute
            .as_deref()
            .and_then(|execute| execute.sub_ability.as_deref())
            .unwrap_or_else(|| panic!("continuation for {oracle:?}"));
        assert!(
            matches!(
                &*counter.effect,
                Effect::PutCounter {
                    target: TargetFilter::LastCreated,
                    ..
                }
            ),
            "{oracle:?} → {:?}",
            counter.effect
        );
    }
}

/// Guard: an OBJECT subject still re-anchors "that creature" to the triggering
/// object (the binding Pip-Boy 3000 relies on).
#[test]
fn object_subject_keeps_the_triggering_source_binding() {
    let parsed = parse_oracle_text(
        "Whenever equipped creature attacks, put a +1/+1 counter on that creature.",
        "Probe",
        &[],
        &["Artifact".to_string()],
        &["Equipment".to_string()],
    );
    let effect = &parsed.triggers[0].execute.as_deref().unwrap().effect;
    assert!(
        matches!(
            &**effect,
            Effect::PutCounter {
                target: TargetFilter::TriggeringSource,
                ..
            }
        ),
        "got {effect:?}"
    );
}

fn back_face(name: &str, oracle: &str) -> BackFaceData {
    let parsed = parse_oracle_text(
        oracle,
        name,
        &[],
        &["Enchantment".to_string()],
        &["Room".to_string()],
    );
    let mut card_types = CardType::default();
    card_types.core_types.push(CoreType::Enchantment);
    card_types.subtypes.push("Room".to_string());
    let mut back = BackFaceData {
        is_swap_snapshot: false,
        trigger_printed_origins: Vec::new(),
        name: name.to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        printed_loyalty: None,
        defense: None,
        card_types,
        mana_cost: ManaCost::generic(0),
        keywords: Vec::new(),
        abilities: Vec::new(),
        trigger_definitions: Default::default(),
        replacement_definitions: Default::default(),
        static_definitions: Default::default(),
        color: Vec::new(),
        printed_ref: None,
        modal: None,
        additional_cost: None,
        strive_cost: None,
        casting_restrictions: Vec::new(),
        casting_options: Vec::new(),
        layout_kind: Some(LayoutKind::Split),
        parse_warnings: vec![],
    };
    for trigger in parsed.triggers {
        back.trigger_definitions.push(trigger);
    }
    back
}

/// Underwater Tunnel on the battlefield (cast half, unlocked), Slimy Aquarium's
/// door locked, `library` cards for manifest dread.
fn aquarium_board(library: usize) -> (GameRunner, ObjectId) {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    for i in 0..library {
        scenario.add_card_to_library_top(P0, &format!("Library {i}"));
    }
    let room = scenario
        .add_enchantment_from_oracle(P0, "Underwater Tunnel", UNDERWATER_TUNNEL)
        .with_subtypes(vec!["Room"])
        .id();
    scenario.with_mana_pool(P0, vec![]);
    let mut runner = scenario.build();
    let state = runner.state_mut();
    let turn = state.turn_number;
    let ts = state.next_timestamp();
    let obj = state.objects.get_mut(&room).unwrap();
    obj.back_face = Some(back_face("Slimy Aquarium", SLIMY_AQUARIUM));
    obj.reset_for_battlefield_entry(turn, ts);
    obj.summoning_sick = false;
    obj.room_unlocks = Some(RoomUnlockState {
        left_unlocked: true,
        right_unlocked: false,
    });
    (runner, room)
}

fn plus_counters(runner: &GameRunner, id: ObjectId) -> u32 {
    runner.state().objects[&id]
        .counters
        .get(&CounterType::Plus1Plus1)
        .copied()
        .unwrap_or(0)
}

/// The relato: unlocking Slimy Aquarium's door manifests dread and the +1/+1
/// counter lands on the manifested creature.
#[test]
fn unlocking_slimy_aquarium_puts_the_counter_on_the_manifested_creature() {
    let (mut runner, room) = aquarium_board(3);
    let manifested = runner.state().players[0].library[0];

    runner
        .act(GameAction::UnlockRoomDoor {
            object_id: room,
            door: RoomDoor::Right,
        })
        .expect("unlock the Slimy Aquarium door");
    runner.advance_until_stack_empty();
    assert!(
        matches!(
            runner.state().waiting_for,
            WaitingFor::ManifestDreadChoice { .. }
        ),
        "manifest dread must pause for the two-card choice, got {:?}",
        runner.state().waiting_for
    );
    runner
        .act(GameAction::SelectCards {
            cards: vec![manifested],
        })
        .expect("choose the card to manifest");
    runner.advance_until_stack_empty();

    assert!(
        runner.state().objects[&manifested].face_down,
        "the chosen card is on the battlefield face down"
    );
    assert_eq!(
        plus_counters(&runner, manifested),
        1,
        "the +1/+1 counter belongs on the manifested creature"
    );
    assert_eq!(plus_counters(&runner, room), 0, "never on the Room");
}

/// CR 609.3: with an empty library nothing is manifested and the counter goes
/// nowhere — in particular, not onto the Room.
#[test]
fn unlocking_slimy_aquarium_with_an_empty_library_places_no_counter() {
    let (mut runner, room) = aquarium_board(0);
    runner
        .act(GameAction::UnlockRoomDoor {
            object_id: room,
            door: RoomDoor::Right,
        })
        .expect("unlock the Slimy Aquarium door");
    runner.advance_until_stack_empty();
    assert_eq!(plus_counters(&runner, room), 0);
}
