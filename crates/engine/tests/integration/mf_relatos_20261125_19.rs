//! Relatos da mesa do MagicFinder, rodada de 25/11/2026 19h.
//!
//! Unearth (Scrapwork Rager, Mask of the Jadecrafter): "If it would leave the
//! battlefield, exile it instead" sumia quando a partida era restaurada
//! (desfazer, recarregar, recuperação da mesa). A reidratação regravava as
//! substituições só com o texto impresso e apagava a instalada pelo Unearth, e a
//! permanente desenterrada voltava ao cemitério — pronta para outro Unearth.

use super::support::shared_card_db;
use engine::database::CardDatabase;
use engine::game::scenario::{GameRunner, GameScenario, P0};
use engine::game::scenario_db::GameScenarioDbExt;
use engine::game::zones::move_to_zone;
use engine::types::game_state::{PersistedGameState, PersistedRestoreFinalization};
use engine::types::identifiers::ObjectId;
use engine::types::mana::{ManaType, ManaUnit};
use engine::types::phase::Phase;
use engine::types::zones::Zone;

const DESTROY_TARGET: &str = "Destroy target creature.";

/// The mesa's save/undo restore pipeline, rehydrating from the card database
/// exactly like the WASM `restoreState`.
fn round_trip_rehydrated(runner: &GameRunner, db: &'static CardDatabase) -> GameRunner {
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

/// Rotting Rats unearthed (its enters discard is a no-op with empty hands), the
/// game saved and restored, and a Doom Blade ready in hand.
fn unearthed_then_restored(restore: bool) -> (GameRunner, ObjectId, ObjectId) {
    let db = shared_card_db().expect("fixture loads");
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let rats = scenario.add_real_card(P0, "Rotting Rats", Zone::Graveyard, db);
    let blade = scenario
        .add_spell_to_library_top(P0, "Doom Blade", false)
        .from_oracle_text(DESTROY_TARGET)
        .id();
    scenario.with_mana_pool(
        P0,
        vec![
            ManaUnit::new(ManaType::Colorless, rats, false, vec![]),
            ManaUnit::new(ManaType::Black, rats, false, vec![]),
        ],
    );
    let mut runner = scenario.build();
    let unearth = runner.state().objects[&rats]
        .abilities
        .iter()
        .position(|ability| ability.activation_zone == Some(Zone::Graveyard))
        .expect("Rotting Rats has Unearth");
    runner.activate(rats, unearth).resolve();
    runner.advance_until_stack_empty();
    assert_eq!(runner.state().objects[&rats].zone, Zone::Battlefield);
    let mut runner = if restore {
        round_trip_rehydrated(&runner, db)
    } else {
        runner
    };
    move_to_zone(runner.state_mut(), blade, Zone::Hand, &mut Vec::new());
    let _ = runner
        .state_mut()
        .add_mana_to_pool(P0, ManaUnit::new(ManaType::Black, rats, false, vec![]));
    (runner, rats, blade)
}

/// REVERT-PROBE: sem levar as substituições de tempo de jogo pela reidratação,
/// os Rotting Rats destruídos depois de restaurar a partida vão ao cemitério.
#[test]
fn unearthed_creature_is_exiled_after_a_save_restore() {
    let (mut runner, rats, blade) = unearthed_then_restored(true);
    let outcome = runner.cast(blade).target_objects(&[rats]).resolve();
    assert_eq!(
        outcome.zone_of(rats),
        Zone::Exile,
        "CR 702.84a: o desenterrado destruído vai ao exílio mesmo depois de restaurar a partida"
    );
}

#[test]
fn unearthed_creature_is_exiled_without_a_restore() {
    let (mut runner, rats, blade) = unearthed_then_restored(false);
    let outcome = runner.cast(blade).target_objects(&[rats]).resolve();
    assert_eq!(outcome.zone_of(rats), Zone::Exile);
}

#[test]
fn restore_twice_keeps_one_exile_replacement() {
    let db = shared_card_db().expect("fixture loads");
    let (runner, rats, _) = unearthed_then_restored(true);
    let runner = round_trip_rehydrated(&runner, db);
    let defs = &runner.state().objects[&rats].base_replacement_definitions;
    let runtime = defs
        .iter()
        .filter(|def| def.expiry.is_some())
        .count();
    assert_eq!(runtime, 1, "a substituição do Unearth não se duplica a cada restauração");
}
