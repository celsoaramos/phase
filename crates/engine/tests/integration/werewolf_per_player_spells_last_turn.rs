//! CR 603.4: the old-werewolf back-face guard "if a player cast two or more
//! spells last turn" (Terror of Kruin Pass, Gatstaf Howler) is true only when
//! ONE player cast two or more spells during the previous turn.
//!
//! Field report (MagicFinder, 2026-10-25, v0.116.0): Terror of Kruin Pass never
//! transformed back after its controller cast two spells — the guard was left
//! unparsed and the whole trigger failed closed.

use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::types::actions::GameAction;
use engine::types::game_state::WaitingFor;
use engine::types::phase::Phase;

/// A test payoff with the werewolf guard verbatim; the body is a life gain so
/// the test observes whether the intervening-if let the trigger through.
const GUARDED: &str = "At the beginning of each upkeep, if a player cast two or more spells last turn, you gain 3 life.";

fn scenario() -> GameScenario {
    let mut scenario = GameScenario::new_n_player(2, 7);
    scenario.at_phase(Phase::PreCombatMain);
    scenario.add_creature_from_oracle(P0, "Guarded Howler", 2, 2, GUARDED);
    scenario
}

/// Pass priority (resolving whatever is on the stack) until the next turn's
/// draw step, then report P0's life.
fn pass_into_next_turn_draw(runner: &mut GameRunner) -> i32 {
    let start_turn = runner.state().turn_number;
    for _ in 0..200 {
        let state = runner.state();
        if state.turn_number > start_turn && state.phase == Phase::Draw {
            return state.players[0].life;
        }
        match state.waiting_for.clone() {
            WaitingFor::Priority { .. } => {
                runner.act(GameAction::PassPriority).expect("pass");
            }
            WaitingFor::DeclareAttackers { .. } => {
                runner.declare_attackers(&[]).expect("no attack");
            }
            other => panic!("unexpected wait: {other:?}"),
        }
    }
    panic!("never reached the next turn's draw step");
}

#[test]
fn one_player_casting_two_spells_satisfies_the_guard() {
    let mut scenario = scenario();
    let a = scenario
        .add_spell_to_hand_from_oracle(P0, "Spell A", false, "You gain 1 life.")
        .id();
    let b = scenario
        .add_spell_to_hand_from_oracle(P0, "Spell B", false, "You gain 1 life.")
        .id();
    let mut runner = scenario.build();
    runner.cast(a).resolve();
    runner.cast(b).resolve();
    let before = runner.state().players[0].life;

    assert_eq!(pass_into_next_turn_draw(&mut runner), before + 3);
    assert_eq!(
        runner.state().most_spells_cast_by_one_player_last_turn,
        Some(2)
    );
}

#[test]
fn one_spell_does_not_satisfy_the_guard() {
    let mut scenario = scenario();
    let a = scenario
        .add_spell_to_hand_from_oracle(P0, "Spell A", false, "You gain 1 life.")
        .id();
    let mut runner = scenario.build();
    runner.cast(a).resolve();
    let before = runner.state().players[0].life;

    assert_eq!(pass_into_next_turn_draw(&mut runner), before);
}

#[test]
fn two_players_casting_one_spell_each_do_not_satisfy_the_guard() {
    let mut scenario = scenario();
    let a = scenario
        .add_spell_to_hand_from_oracle(P0, "Spell A", false, "You gain 1 life.")
        .id();
    let b = scenario
        .add_spell_to_hand_from_oracle(P1, "Spell B", true, "You gain 1 life.")
        .id();
    let mut runner = scenario.build();
    runner.cast(a).resolve();
    runner.state_mut().priority_player = P1;
    runner.state_mut().waiting_for = WaitingFor::Priority { player: P1 };
    runner.cast(b).resolve();
    assert_eq!(
        runner.state().spells_cast_this_turn,
        2,
        "two spells were cast this turn"
    );
    let before = runner.state().players[0].life;

    assert_eq!(
        pass_into_next_turn_draw(&mut runner),
        before,
        "one spell each is two spells table-wide, but no ONE player cast two"
    );
    assert_eq!(
        runner.state().most_spells_cast_by_one_player_last_turn,
        Some(1)
    );
}
