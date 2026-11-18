//! Relatos da mesa de 18/11/2026 (rodada das 14h UTC).
//!
//! Tireless Tracker + Lotus Cobra no campo e uma Vineglimmer Snarl jogada: a Snarl
//! pausa a entrada no "you may reveal a Forest or Island card from your hand" e,
//! respondida a escolha (revelando OU recusando), o landfall nunca disparava — a
//! entrada não era guardada para depois da escolha (CR 603.2 + CR 614.12a).

use super::support::shared_card_db;
use engine::game::scenario::{GameRunner, GameScenario, P0};
use engine::game::scenario_db::GameScenarioDbExt;
use engine::types::actions::GameAction;
use engine::types::game_state::WaitingFor;
use engine::types::identifiers::ObjectId;
use engine::types::phase::Phase;
use engine::types::zones::Zone;

const SNARL: &str = "As this land enters, you may reveal a Forest or Island card from your hand. If you don't, this land enters tapped.\n{T}: Add {G} or {U}.";

struct Board {
    runner: GameRunner,
    snarl: ObjectId,
    stream: ObjectId,
}

fn board() -> Option<Board> {
    let db = shared_card_db()?;
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.add_real_card(P0, "Tireless Tracker", Zone::Battlefield, db);
    let snarl = {
        let mut b = scenario.add_land_to_hand(P0, "Vineglimmer Snarl");
        b.from_oracle_text(SNARL);
        b.id()
    };
    let stream = {
        let mut b = scenario.add_land_to_hand(P0, "Prairie Stream");
        b.with_subtypes(vec!["Plains", "Island"]);
        b.id()
    };
    Some(Board { runner: scenario.build(), snarl, stream })
}

fn play_snarl_and_answer(b: &mut Board, reveal: bool) {
    let card_id = b.runner.state().objects[&b.snarl].card_id;
    b.runner
        .act(GameAction::PlayLand { object_id: b.snarl, card_id })
        .expect("the Snarl is playable");
    assert!(
        matches!(b.runner.state().waiting_for, WaitingFor::RevealChoice { .. }),
        "the Snarl asks for the optional reveal, got {:?}",
        b.runner.state().waiting_for
    );
    let cards = if reveal { vec![b.stream] } else { vec![] };
    b.runner
        .act(GameAction::SelectCards { cards })
        .expect("the reveal answer is accepted");
    b.runner.advance_until_stack_empty();
}

fn clues(runner: &GameRunner) -> usize {
    runner
        .state()
        .battlefield
        .iter()
        .filter(|id| runner.state().objects[id].name == "Clue")
        .count()
}

/// REVERT-PROBE: sem a captura da entrada no `RevealChoice`, 0 Pistas.
#[test]
fn snarl_revealed_still_fires_landfall() {
    let Some(mut b) = board() else { return };
    play_snarl_and_answer(&mut b, true);
    assert!(!b.runner.state().objects[&b.snarl].tapped, "revealed: enters untapped");
    assert_eq!(clues(&b.runner), 1, "Tireless Tracker investigates once");
}

#[test]
fn snarl_declined_still_fires_landfall() {
    let Some(mut b) = board() else { return };
    play_snarl_and_answer(&mut b, false);
    assert!(b.runner.state().objects[&b.snarl].tapped, "declined: enters tapped");
    assert_eq!(clues(&b.runner), 1, "Tireless Tracker investigates once");
}
