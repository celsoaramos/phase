//! Akiri, Fearless Voyager — "Whenever you attack a player with one or more
//! equipped creatures, draw a card."
//!
//! The `with one or more equipped creatures` tail used to be dropped by the
//! parser, leaving a bare "whenever you attack": Akiri drew a card on every
//! attack, equipped or not (CR 508.3e + CR 508.1).

use engine::game::game_object::AttachTarget;

use super::rules::{AttackTarget, GameRunner, GameScenario, ObjectId, Phase, P0, P1};

const AKIRI: &str =
    "Whenever you attack a player with one or more equipped creatures, draw a card.";

const BONESPLITTER: &str = "Equipped creature gets +2/+0.\nEquip {1}";

/// Hand size change after P0 attacks P1 with a plain creature, with or without
/// an Equipment attached to it.
fn cards_drawn_on_attack(equipped: bool) -> usize {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.with_library_top(P0, &["Filler A", "Filler B"]);
    scenario.with_library_top(P1, &["Filler C"]);
    scenario.add_creature_from_oracle(P0, "Akiri, Fearless Voyager", 3, 3, AKIRI);
    let bearer = scenario.add_creature(P0, "Bearer", 2, 2).id();
    let blade = scenario
        .add_artifact_from_oracle(P0, "Bonesplitter", BONESPLITTER)
        .with_subtypes(vec!["Equipment"])
        .id();
    let mut runner = scenario.build();
    if equipped {
        attach(&mut runner, blade, bearer);
    }
    let before = runner.state().players[0].hand.len();
    runner.pass_both_players();
    runner
        .declare_attackers(&[(bearer, AttackTarget::Player(P1))])
        .expect("DeclareAttackers should succeed");
    // CR 508.2 + CR 603.3: the attack trigger goes on the stack after
    // declaration; resolve it (P1 has no creatures, so no blockers step).
    runner.advance_until_stack_empty();
    runner.state().players[0].hand.len() - before
}

fn attach(runner: &mut GameRunner, equipment: ObjectId, host: ObjectId) {
    let state = runner.state_mut();
    state.objects.get_mut(&equipment).unwrap().attached_to = Some(AttachTarget::Object(host));
    state
        .objects
        .get_mut(&host)
        .unwrap()
        .attachments
        .push(equipment);
    state.layers_dirty.mark_full();
}

#[test]
fn attacking_with_an_unequipped_creature_draws_nothing() {
    assert_eq!(cards_drawn_on_attack(false), 0);
}

#[test]
fn attacking_with_an_equipped_creature_draws_one() {
    assert_eq!(cards_drawn_on_attack(true), 1);
}
