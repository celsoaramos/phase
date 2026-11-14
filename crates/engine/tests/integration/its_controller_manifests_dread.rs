//! CR 701.62a + CR 608.2c: "Destroy target creature. Its controller manifests
//! dread." (Unwanted Remake) — the player who manifests is the destroyed
//! creature's controller, not the caster.
//!
//! `Effect::ManifestDread` has no player slot, so the resolver acts for the
//! ability's controller. Without lifting the "its controller" subject to the
//! clause's `player_scope` (the same channel Fateful Absence's "its controller
//! investigates" uses), the CASTER looked at the top two cards of their OWN
//! library and got the face-down 2/2 — the relato of 29/10/2026.
//!
//! Cards are built from Oracle text (CI has no card database).

use engine::game::scenario::{GameScenario, P0, P1};
use engine::types::actions::GameAction;
use engine::types::game_state::WaitingFor;
use engine::types::mana::ManaCost;
use engine::types::phase::Phase;
use engine::types::zones::Zone;

const UNWANTED_REMAKE: &str = "Destroy target creature. Its controller manifests dread.";

#[test]
fn the_destroyed_creatures_controller_manifests_dread() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    for i in 0..2 {
        scenario.add_card_to_library_top(P0, &format!("Caster Library {i}"));
        scenario.add_card_to_library_top(P1, &format!("Victim Library {i}"));
    }
    let victim = scenario.add_vanilla(P1, 3, 3);
    let remake = scenario
        .add_spell_to_hand_from_oracle(P0, "Unwanted Remake", true, UNWANTED_REMAKE)
        .with_mana_cost(ManaCost::generic(0))
        .id();
    let mut runner = scenario.build();
    let caster_library = runner.state().players[0].library.clone();
    let victim_top = runner.state().players[1].library[0];

    runner.cast(remake).target_object(victim).resolve();
    runner.advance_until_stack_empty();

    assert_eq!(runner.state().objects[&victim].zone, Zone::Graveyard);
    match &runner.state().waiting_for {
        WaitingFor::ManifestDreadChoice { player, .. } => assert_eq!(
            *player, P1,
            "the destroyed creature's controller chooses the card to manifest"
        ),
        other => panic!("manifest dread must pause for the two-card choice, got {other:?}"),
    }
    runner
        .act(GameAction::SelectCards {
            cards: vec![victim_top],
        })
        .expect("the victim picks the card to manifest");
    runner.advance_until_stack_empty();

    let manifested = &runner.state().objects[&victim_top];
    assert_eq!(manifested.zone, Zone::Battlefield);
    assert!(manifested.face_down, "the manifested card enters face down");
    assert_eq!(manifested.controller, P1, "the 2/2 enters under the victim's control");
    assert_eq!(
        runner.state().players[0].library,
        caster_library,
        "the caster's library is untouched"
    );
    assert_eq!(
        runner.state().players[1].library.len(),
        0,
        "both of the victim's cards left the library (one manifested, one milled)"
    );
}

/// Counter-direction: a bare "manifest dread" on the same spell shape keeps the
/// caster default — the lift must not over-apply.
#[test]
fn a_bare_manifest_dread_stays_with_the_caster() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    for i in 0..2 {
        scenario.add_card_to_library_top(P0, &format!("Caster Library {i}"));
        scenario.add_card_to_library_top(P1, &format!("Victim Library {i}"));
    }
    let victim = scenario.add_vanilla(P1, 3, 3);
    let spell = scenario
        .add_spell_to_hand_from_oracle(
            P0,
            "Bare Dread",
            true,
            "Destroy target creature. Manifest dread.",
        )
        .with_mana_cost(ManaCost::generic(0))
        .id();
    let mut runner = scenario.build();

    runner.cast(spell).target_object(victim).resolve();
    runner.advance_until_stack_empty();

    match &runner.state().waiting_for {
        WaitingFor::ManifestDreadChoice { player, .. } => assert_eq!(*player, P0),
        other => panic!("manifest dread must pause for the two-card choice, got {other:?}"),
    }
}
