//! CR 603.7a + CR 603.4 + CR 400.7 + CR 111.1: Glimpse the Impossible —
//! "Exile the top three cards of your library. You may play those cards this
//! turn. At the beginning of the next end step, if any of those cards remain
//! exiled, put them into your graveyard, then create a 0/1 colorless Eldrazi
//! Spawn creature token for each card put into your graveyard this way. Those
//! tokens have \"Sacrifice this token: Add {C}.\""
//!
//! Before: the end-step instruction lowered to an unbound graveyard move (the
//! "if any of those cards remain exiled" gate sent the body down the
//! play-from-exile route, swallowing the token clause) and was demoted to
//! `delayed_unplayed_exile_sweep`; the "Those tokens have" rider bound to the
//! exiled cards at spell resolution. Nothing moved and no token was made.

use engine::game::scenario::{GameScenario, P0};
use engine::parser::oracle::parse_oracle_text;
use engine::types::identifiers::ObjectId;
use engine::types::phase::Phase;
use engine::types::zones::Zone;

const GLIMPSE: &str = "Exile the top three cards of your library. You may play those cards this turn. At the beginning of the next end step, if any of those cards remain exiled, put them into your graveyard, then create a 0/1 colorless Eldrazi Spawn creature token for each card put into your graveyard this way. Those tokens have \"Sacrifice this token: Add {C}.\"";

fn spawns(runner: &engine::game::scenario::GameRunner) -> Vec<ObjectId> {
    runner
        .state()
        .battlefield
        .iter()
        .copied()
        .filter(|id| runner.state().objects[id].name == "Eldrazi Spawn")
        .collect()
}

/// Cast Glimpse, then (optionally) take `played` of the exiled cards out of exile
/// before the end step, and run to the end of the delayed trigger.
fn run(played: usize) -> (engine::game::scenario::GameRunner, Vec<ObjectId>) {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    // `add_card_to_library_top` puts each card ON TOP: the one that must stay
    // in the library goes first.
    scenario.add_card_to_library_top(P0, "Card Below");
    let top: Vec<ObjectId> = ["Card C", "Card B", "Card A"]
        .iter()
        .map(|n| scenario.add_card_to_library_top(P0, n))
        .collect();
    let glimpse = scenario
        .add_spell_to_hand_from_oracle(P0, "Glimpse the Impossible", false, GLIMPSE)
        .id();
    let mut runner = scenario.build();
    runner.cast(glimpse).free_cast().resolve();
    for id in &top {
        assert_eq!(runner.state().objects[id].zone, Zone::Exile, "the top three are exiled");
    }
    let mut events = Vec::new();
    for id in top.iter().take(played) {
        engine::game::zones::move_to_zone(runner.state_mut(), *id, Zone::Hand, &mut events);
    }
    runner.advance_to_end_step();
    runner.advance_until_stack_empty();
    (runner, top)
}

#[test]
fn glimpse_parses_with_no_unimplemented_sweep() {
    let parsed = parse_oracle_text(GLIMPSE, "Glimpse the Impossible", &[], &["Sorcery".to_string()], &[]);
    let dbg = format!("{parsed:?}");
    assert!(!dbg.contains("Unimplemented"), "no residual gap: {dbg}");
}

#[test]
fn unplayed_cards_go_to_the_graveyard_as_eldrazi_spawn() {
    let (runner, top) = run(0);
    for id in &top {
        assert_eq!(
            runner.state().objects[id].zone,
            Zone::Graveyard,
            "every card that remained exiled is put into the graveyard"
        );
    }
    let spawns = spawns(&runner);
    assert_eq!(spawns.len(), 3, "one Eldrazi Spawn per card put into the graveyard");
    for id in &spawns {
        let obj = &runner.state().objects[id];
        assert_eq!((obj.power, obj.toughness), (Some(0), Some(1)));
        let abilities = format!("{:?}", obj.abilities);
        assert!(
            abilities.contains("Mana") && abilities.contains("Sacrifice"),
            "each token has \"Sacrifice this token: Add {{C}}.\": {abilities}"
        );
    }
}

#[test]
fn a_card_that_left_exile_is_not_counted() {
    let (runner, top) = run(1);
    assert_eq!(runner.state().objects[&top[0]].zone, Zone::Hand, "the played card stays where it went");
    assert_eq!(spawns(&runner).len(), 2, "only the two cards still exiled make tokens");
}

#[test]
fn nothing_left_in_exile_makes_nothing() {
    let (runner, _) = run(3);
    assert!(spawns(&runner).is_empty(), "no card remained exiled → no token");
}

