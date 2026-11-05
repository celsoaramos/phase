//! MagicFinder (2026-11-04): time `score_candidates_for_parallel_worker` on a
//! restored table state — the exact call the browser makes for every AI
//! decision (`get_ai_scored_candidates`).
//!
//!   cargo run --release -p phase-ai --example bigboard_score -- <state.json> <card-data.json> [seat] [levels] [reps]
use engine::types::game_state::PersistedGameState;
use engine::types::player::PlayerId;
use phase_ai::config::{create_config_for_players, AiDifficulty, Platform};
use phase_ai::search::score_candidates_for_parallel_worker;
use std::time::Instant;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let state_path = &a[1];
    let db_path = &a[2];
    let seat = PlayerId(a.get(3).and_then(|s| s.parse().ok()).unwrap_or(1));
    let levels = a.get(4).cloned().unwrap_or_else(|| "easy,veryhard".into());
    let reps: usize = a.get(5).and_then(|s| s.parse().ok()).unwrap_or(1);

    let t = Instant::now();
    let db = (db_path != "-").then(|| {
        std::sync::Arc::new(
            engine::database::card_db::CardDatabase::from_json_str(&std::fs::read_to_string(db_path).unwrap())
                .expect("card db"),
        )
    });
    eprintln!("card db {:.1}s", t.elapsed().as_secs_f64());
    let raw = std::fs::read_to_string(state_path).unwrap();
    let persisted: PersistedGameState = serde_json::from_str(&raw).expect("persisted");
    let mut state = persisted.into_game_state().expect("restore");
    if let Some(db) = &db {
        engine::game::rehydrate_game_from_card_db_with_finalization(
            &mut state,
            db,
            engine::game::CardDbRehydrationFinalization::Immediate,
        );
        engine::game::install_card_db(&mut state, db.clone());
    }
    engine::game::layers::flush_layers(&mut state);
    eprintln!(
        "T{} {:?} waiting {:?} battlefield {}",
        state.turn_number,
        state.phase,
        std::mem::discriminant(&state.waiting_for),
        state.battlefield.len()
    );
    for lvl in levels.split(',') {
        let difficulty = AiDifficulty::from_label(lvl);
        for r in 0..reps {
            let mut s = state.clone();
            s.rng_seed = 1;
            s.rng = engine::types::game_state::GameRng::seed_from_u64(1);
            s.rng_word_pos = 0;
            let mut config = create_config_for_players(difficulty, Platform::Wasm, s.players.len() as u8);
            // MEASURE=1: no wall clock (deterministic) — compares two builds by verdict, not by time.
            if std::env::var("MEASURE").is_ok() {
                config = config.into_measurement(1);
            }
            let t = Instant::now();
            let scored = score_candidates_for_parallel_worker(&s, seat, &config, None);
            let ms = t.elapsed().as_secs_f64() * 1000.0;
            let summary: Vec<String> = scored
                .iter()
                .map(|(act, sc)| format!("{}:{:.2}", serde_json::to_value(act).unwrap()["type"].as_str().unwrap_or("?"), sc))
                .collect();
            println!("{lvl} #{r}: {ms:.0} ms · {}", summary.join(" "));
        }
    }
}
