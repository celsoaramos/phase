//! Grimoire Thief (MagicFinder field report, 2026-10-20).
//!
//! "Whenever Grimoire Thief becomes tapped, exile the top three cards of target
//! opponent's library face down.
//! You may look at cards exiled with Grimoire Thief.
//! {U}, Sacrifice Grimoire Thief: Turn all cards exiled with Grimoire Thief face
//! up. Counter all spells with those names."
//!
//! Two parser defects: "with those names" was dropped, so the activation
//! countered EVERY spell on the stack (CR 701.6 + CR 201.2), and "You may look at
//! cards exiled with ~" was unparsed, so the controller could never see the
//! face-down exiles (CR 406.3).

use engine::game::combat::AttackTarget;
use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::game::visibility::filter_state_for_viewer;
use engine::game::zones::create_object;
use engine::types::actions::GameAction;
use engine::types::card_type::CoreType;
use engine::types::game_state::{StackEntry, StackEntryKind};
use engine::types::identifiers::{CardId, ObjectId};
use engine::types::mana::{ManaType, ManaUnit};
use engine::types::phase::Phase;
use engine::types::player::PlayerId;
use engine::types::zones::Zone;

const GRIMOIRE_THIEF_ORACLE: &str = "Whenever Grimoire Thief becomes tapped, exile the top three cards of target opponent's library face down.\nYou may look at cards exiled with Grimoire Thief.\n{U}, Sacrifice Grimoire Thief: Turn all cards exiled with Grimoire Thief face up. Counter all spells with those names.";

/// Grimoire Thief attacks; its tapped trigger exiles P1's top three cards.
/// Returns the runner, the Thief and the exiled card ids (top first).
fn thief_after_attack() -> (GameRunner, ObjectId, Vec<ObjectId>) {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.add_card_to_library_top(P1, "Bottom Card");
    let third = scenario.add_card_to_library_top(P1, "Island");
    let second = scenario.add_card_to_library_top(P1, "Counterspell");
    let first = scenario.add_card_to_library_top(P1, "Lightning Bolt");
    let thief = scenario
        .add_creature_from_oracle(P0, "Grimoire Thief", 2, 2, GRIMOIRE_THIEF_ORACLE)
        .id();
    scenario.with_mana_pool(
        P0,
        vec![ManaUnit::new(ManaType::Blue, ObjectId(0), false, vec![])],
    );
    let mut runner = scenario.build();
    runner.advance_to_combat();
    runner
        .declare_attackers(&[(thief, AttackTarget::Player(P1))])
        .expect("attack with Grimoire Thief");
    runner.advance_until_stack_empty();
    (runner, thief, vec![first, second, third])
}

/// Put a named instant on the stack the way the engine does (entry id == object id).
fn push_spell(runner: &mut GameRunner, controller: PlayerId, name: &str) -> ObjectId {
    let state = runner.state_mut();
    let id = create_object(
        state,
        CardId(state.next_object_id),
        controller,
        name.to_string(),
        Zone::Stack,
    );
    let card_id = {
        let obj = state.objects.get_mut(&id).unwrap();
        obj.card_types.core_types.push(CoreType::Instant);
        obj.base_card_types = obj.card_types.clone();
        obj.card_id
    };
    state.stack.push_back(StackEntry {
        id,
        source_id: id,
        controller,
        kind: StackEntryKind::Spell {
            card_id,
            ability: None,
            casting_variant: Default::default(),
            actual_mana_spent: 0,
        },
    });
    id
}

#[test]
fn tapping_exiles_three_face_down_and_only_the_thief_controller_may_look() {
    let (runner, _thief, exiled) = thief_after_attack();
    for &id in &exiled {
        let obj = &runner.state().objects[&id];
        assert_eq!(obj.zone, Zone::Exile, "the top three cards are exiled");
        assert!(obj.face_down, "exiled face down");
    }
    let mine = filter_state_for_viewer(runner.state(), P0);
    let theirs = filter_state_for_viewer(runner.state(), P1);
    assert_eq!(
        mine.objects[&exiled[0]].name, "Lightning Bolt",
        "\"You may look at cards exiled with ~\": the Thief's controller sees them"
    );
    assert_ne!(
        theirs.objects[&exiled[0]].name, "Lightning Bolt",
        "CR 406.3: the opponent may not look at the face-down exiles"
    );
}

#[test]
fn sacrifice_counters_only_spells_with_the_exiled_names() {
    let (mut runner, thief, _exiled) = thief_after_attack();
    let unrelated = push_spell(&mut runner, P1, "Giant Growth");
    let bolt = push_spell(&mut runner, P1, "Lightning Bolt");
    let state = runner.state_mut();
    // Mana empties between steps (CR 500.4): float the {U} at the moment of use.
    state.players[0]
        .mana_pool
        .add(ManaUnit::new(ManaType::Blue, ObjectId(0), false, vec![]));
    state.priority_player = P0;
    state.waiting_for = engine::types::game_state::WaitingFor::Priority { player: P0 };

    runner
        .act(GameAction::ActivateAbility {
            payment_mode: engine::types::game_state::CastPaymentMode::Auto,
            source_id: thief,
            ability_index: 0,
        })
        .expect("activate Grimoire Thief");
    runner.resolve_top();

    let on_stack: Vec<ObjectId> = runner.state().stack.iter().map(|entry| entry.id).collect();
    assert!(
        !on_stack.contains(&bolt),
        "Lightning Bolt shares a name with an exiled card: countered"
    );
    assert!(
        on_stack.contains(&unrelated),
        "Giant Growth shares no name with the exiled cards: it stays on the stack"
    );
}
