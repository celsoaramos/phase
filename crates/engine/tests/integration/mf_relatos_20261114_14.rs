//! Field report of 2026-11-14 (MagicFinder mesa, engine v0.124.0):
//!
//! * Scion of the Ur-Dragon — "becomes a copy of that card until end of turn"
//!   stayed a copy of the Dragon for the rest of the game. The copy itself ended
//!   on time in an uninterrupted game; it became permanent whenever the game was
//!   restored (undo, reload, crash recovery) while the copy was active:
//!   rehydration re-stamped the object's base characteristics from its display
//!   pointer, which the copy effect had pointed at the copied card. Mirage
//!   Mirror, Shifting Woodland, Cytoshape and Mirrorweave share the defect.

use engine::game::scenario::{GameRunner, GameScenario, P0};
use engine::game::scenario_db::GameScenarioDbExt;
use engine::types::actions::GameAction;
use engine::types::card_type::CoreType;
use engine::types::game_state::WaitingFor;
use engine::types::mana::{ManaType, ManaUnit};
use engine::types::phase::Phase;
use engine::types::zones::Zone;

use crate::support::shared_card_db;

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

fn mirror_copying_dragon(
    db: &'static engine::database::CardDatabase,
) -> (GameRunner, engine::types::identifiers::ObjectId) {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let mirror = scenario.add_real_card(P0, "Mirage Mirror", Zone::Battlefield, db);
    let dragon = scenario.add_real_card(P0, "Shivan Dragon", Zone::Battlefield, db);
    scenario.with_mana_pool(
        P0,
        (0..2)
            .map(|_| ManaUnit::new(ManaType::Colorless, mirror, false, vec![]))
            .collect(),
    );
    let mut runner = scenario.build();
    engine::game::rehydrate_game_from_card_db(runner.state_mut(), db);
    runner.activate(mirror, 0).target_object(dragon).resolve();
    assert_eq!(
        runner.state().objects[&mirror].name,
        "Shivan Dragon",
        "the Mirror is a copy of the Dragon this turn"
    );
    (runner, mirror)
}

/// Plays the rest of the turn out (no attack) and stops in the next turn, past
/// this turn's cleanup step.
fn pass_to_next_turn(runner: &mut GameRunner) {
    let turn = runner.state().turn_number;
    for _ in 0..80 {
        if runner.state().turn_number > turn {
            return;
        }
        let action = match runner.state().waiting_for.clone() {
            WaitingFor::DeclareAttackers { .. } => GameAction::DeclareAttackers {
                attacks: Vec::new(),
                bands: Vec::new(),
            },
            WaitingFor::Priority { .. } => GameAction::PassPriority,
            other => panic!("unexpected wait while ending the turn: {other:?}"),
        };
        runner.act(action).expect("ending the turn");
    }
    panic!("the turn never ended");
}

fn assert_is_mirror_again(runner: &GameRunner, mirror: engine::types::identifiers::ObjectId) {
    let obj = &runner.state().objects[&mirror];
    assert_eq!(obj.name, "Mirage Mirror", "the copy effect ends at cleanup");
    assert!(
        !obj.card_types.core_types.contains(&CoreType::Creature),
        "back to a noncreature artifact"
    );
    assert_eq!(obj.power, None, "no copied power left behind");
}

/// CR 514.2: baseline — without a restore the copy ends at cleanup.
#[test]
fn temporary_copy_ends_at_cleanup() {
    let db = shared_card_db().expect("fixture loads");
    let (mut runner, mirror) = mirror_copying_dragon(db);
    pass_to_next_turn(&mut runner);
    assert_is_mirror_again(&runner, mirror);
}

/// CR 707.2 + CR 514.2: a game restored while the copy is active keeps the
/// copy for the rest of the turn and still ends it at cleanup.
#[test]
fn temporary_copy_still_ends_after_save_restore() {
    let db = shared_card_db().expect("fixture loads");
    let (runner, mirror) = mirror_copying_dragon(db);
    let mut runner = round_trip_rehydrated(&runner, db);
    engine::game::layers::evaluate_layers(runner.state_mut());
    assert_eq!(
        runner.state().objects[&mirror].name,
        "Shivan Dragon",
        "the restore keeps the copy while its duration lasts"
    );
    assert_eq!(
        runner.state().objects[&mirror].base_name,
        "Mirage Mirror",
        "the restore must not re-base the object onto the copied card"
    );
    pass_to_next_turn(&mut runner);
    assert_is_mirror_again(&runner, mirror);
}
