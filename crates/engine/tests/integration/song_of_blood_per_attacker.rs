//! Song of Blood — a multi-fire delayed "whenever a creature attacks this turn"
//! trigger fires once PER attacking creature.
//!
//! Oracle: "Mill four cards. Whenever a creature attacks this turn, it gets
//! +1/+0 until end of turn for each creature card in your graveyard."
//!
//! CR 603.2c + CR 508.1: declaring several attackers is one event with several
//! occurrences, so the delayed trigger fires for each attacker and "it" is that
//! attacker. Before the fix the delayed trigger fired ONCE on the aggregate
//! `AttackersDeclared`; with two or more attackers `TriggeringSource` resolved to
//! nothing and no attacker got the bonus (field report, 2026-09-29). The parser
//! also bound "it" to `SelfRef` (the sorcery) instead of the attacking creature,
//! so even a lone attacker got nothing.

use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::types::identifiers::ObjectId;
use engine::types::phase::Phase;

use super::rules::run_combat;

const SONG_ORACLE: &str = "Mill four cards. Whenever a creature attacks this turn, it \
    gets +1/+0 until end of turn for each creature card in your graveyard.";

/// Three creature cards already in the graveyard; the mill hits four non-creature
/// cards, so every attacker must get exactly +3/+0.
fn setup(attackers: usize) -> (GameRunner, Vec<ObjectId>) {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    for name in ["Dead A", "Dead B", "Dead C"] {
        scenario.add_creature_to_graveyard(P0, name, 1, 1);
    }
    for name in ["Lib 1", "Lib 2", "Lib 3", "Lib 4", "Lib 5"] {
        scenario.add_card_to_library_top(P0, name);
    }
    let ids = (0..attackers)
        .map(|i| {
            scenario
                .add_creature(P0, &format!("Attacker {i}"), 1, 1)
                .id()
        })
        .collect();
    let song = scenario
        .add_spell_to_hand_from_oracle(P0, "Song of Blood", false, SONG_ORACLE)
        .id();
    let mut runner = scenario.build();
    let _ = runner.cast(song).resolve();
    runner.advance_until_stack_empty();
    (runner, ids)
}

#[test]
fn every_attacker_gets_the_bonus() {
    let (mut runner, attackers) = setup(4);
    let life_before = runner.life(P1);
    run_combat(&mut runner, attackers, vec![]);
    runner.advance_until_stack_empty();
    // 4 attackers × (1 base + 3 creature cards) — was 4 before the fix.
    assert_eq!(runner.life(P1), life_before - 16);
}

#[test]
fn a_single_attacker_still_gets_the_bonus_once() {
    let (mut runner, attackers) = setup(1);
    let life_before = runner.life(P1);
    run_combat(&mut runner, attackers, vec![]);
    runner.advance_until_stack_empty();
    assert_eq!(runner.life(P1), life_before - 4);
}
