//! ai-ladder — head-to-head ladder measurement with a DIFFERENT AI config per side.
//!
//! `ai-duel` puts the same config in both seats, so it measures deck × deck at a
//! fixed difficulty. This binary measures config × config: every seed is played
//! twice, once with side A in seat 0 and once in seat 1 (paired seeds), so deck
//! and seat advantage cancel and what is left is the difference between the two
//! configs.
//!
//! ```text
//! ai-ladder <data-dir> --a veryhard:wasm --b medium:wasm \
//!     --matchups blue-mirror,red-mirror --games 40 [--decks decks.json] [--json out.json]
//! ```
//!
//! SPEC is `difficulty[:native|wasm][,key=value...]`. Keys override single fields
//! on top of the preset (`temp`, `patience`, `risk`, `stabilize`, `search=0|1`,
//! `depth`, `det=K` (hidden-info samples; 0 = sees the opponent's hand), `strategy=0|1`, `gate=0|1`, `stack=0|1`, `mf=0|1` = all three) — so a code change gated behind a config
//! field can be A/B'd in ONE binary against the same seeds.
//!
//! `--decks` adds named decks (`{"name": ["4 Card", "Card", ...]}`); a matchup
//! `x-vs-y` whose sides are both deck names uses them instead of the duel suite.
//! Given a DIRECTORY, every `*.json` under it in the duel_decks snapshot shape
//! (`{name, main: [{name, count}]}`) is registered under its file stem
//! (`boros-energy`), so `--matchups boros-energy-vs-affinity` works.
//!
//! `--audit` plays one AI decision per step and runs every decision through
//! `phase_ai::blunder_audit` — a table of blunders per game per side follows
//! the win table (and `--json` carries the counts).

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use std::collections::{HashMap, HashSet};
use std::panic::AssertUnwindSafe;
use std::path::PathBuf;

use rayon::prelude::*;

use phase_ai::auto_play::{run_ai_actions, run_ai_actions_bounded};
use phase_ai::blunder_audit::{audit_step, AuditTally, Blunder};
use phase_ai::config::{create_config_for_players, AiConfig, AiDifficulty, Platform};
use phase_ai::duel_suite::{all_matchups, resolve_deck_ref};

use engine::database::CardDatabase;
use engine::game::deck_loading::{
    load_and_hydrate_decks, resolve_deck_list, DeckList, DeckPayload, PlayerDeckList,
};
use engine::game::engine::start_game_skip_mulligan;
use engine::game::turn_control;
use engine::types::card_type::CoreType;
use engine::types::game_state::{GameState, WaitingFor};
use engine::types::player::PlayerId;

const MAX_TURNS: u32 = 60;

struct Side {
    label: String,
    config: AiConfig,
}

fn parse_spec(spec: &str) -> Result<Side, String> {
    let mut parts = spec.split(',');
    let head = parts.next().unwrap_or("medium");
    let (diff, platform) = match head.split_once(':') {
        Some((d, p)) => (d, p),
        None => (head, "wasm"),
    };
    let platform = match platform {
        "native" => Platform::Native,
        "wasm" => Platform::Wasm,
        other => return Err(format!("unknown platform '{other}'")),
    };
    let mut config = create_config_for_players(AiDifficulty::from_label(diff), platform, 2);
    for kv in parts {
        let (k, v) = kv
            .split_once('=')
            .ok_or_else(|| format!("override '{kv}' is not key=value"))?;
        let num = || -> Result<f64, String> {
            v.parse::<f64>()
                .map_err(|_| format!("override '{kv}': '{v}' is not a number"))
        };
        match k {
            "temp" => config.temperature = num()?,
            "patience" => config.profile.interaction_patience = num()?,
            "risk" => config.profile.risk_tolerance = num()?,
            "stabilize" => config.profile.stabilize_bias = num()?,
            "search" => config.search.enabled = num()? != 0.0,
            "depth" => config.search.max_depth = num()? as u32,
            "det" => config.search.determinization_samples = num()? as u32,
            "strategy" => config.strategic.archetype_profile_everywhere = num()? != 0.0,
            "gate" => config.strategic.counter_risk_gate = num()? != 0.0,
            "stack" => config.strategic.stack_spell_credit = num()? != 0.0,
            "mf" => {
                let on = num()? != 0.0;
                config.strategic = phase_ai::config::StrategicConfig {
                    archetype_profile_everywhere: on,
                    counter_risk_gate: on,
                    stack_spell_credit: on,
                }
            }
            other => return Err(format!("unknown override key '{other}'")),
        }
    }
    Ok(Side {
        label: spec.to_string(),
        config,
    })
}

fn expand_deck(lines: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for line in lines {
        let line = line.trim();
        match line.split_once(' ') {
            Some((n, name)) if n.parse::<usize>().is_ok() => {
                for _ in 0..n.parse::<usize>().unwrap() {
                    out.push(name.trim().to_string());
                }
            }
            _ => out.push(line.to_string()),
        }
    }
    out
}

/// Load `--decks`: a JSON map `{name: [lines]}`, or a directory of duel_decks
/// snapshots (`{name, main: [{name, count}]}`) registered by file stem.
fn load_decks(path: &PathBuf) -> HashMap<String, Vec<String>> {
    if path.is_dir() {
        let mut out = HashMap::new();
        let mut files = Vec::new();
        collect_json_files(path, &mut files);
        for file in files {
            let text = std::fs::read_to_string(&file).expect("read deck file");
            match deck_lines_from_snapshot(&text) {
                Some(lines) => {
                    let stem = file
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or_default()
                        .to_string();
                    if out.insert(stem.clone(), lines).is_some() {
                        eprintln!("--decks: duplicate deck stem '{stem}' ({})", file.display());
                    }
                }
                None => eprintln!(
                    "--decks: {} is not a {{name, main:[{{name,count}}]}} snapshot, skipped",
                    file.display()
                ),
            }
        }
        return out;
    }
    serde_json::from_str(&std::fs::read_to_string(path).expect("read --decks"))
        .expect("--decks must be {name: [lines]} or a directory of deck snapshots")
}

fn collect_json_files(dir: &PathBuf, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            collect_json_files(&p, out);
        } else if p.extension().and_then(|e| e.to_str()) == Some("json") {
            out.push(p);
        }
    }
}

/// `{name, main: [{name, count}]}` -> `["4 Card", ...]` (the `expand_deck` shape).
fn deck_lines_from_snapshot(text: &str) -> Option<Vec<String>> {
    let v: serde_json::Value = serde_json::from_str(text).ok()?;
    let main = v.get("main")?.as_array()?;
    let mut lines = Vec::with_capacity(main.len());
    for card in main {
        let name = card.get("name")?.as_str()?;
        let count = card.get("count").and_then(|c| c.as_u64()).unwrap_or(1);
        lines.push(format!("{count} {name}"));
    }
    Some(lines)
}

fn payload_for(
    db: &CardDatabase,
    id: &str,
    decks: &HashMap<String, Vec<String>>,
) -> Result<DeckPayload, String> {
    let (p0, p1) = if let Some(spec) = all_matchups().iter().find(|m| m.id == id) {
        (
            resolve_deck_ref(&spec.p0).map_err(|e| format!("{id} p0: {e}"))?,
            resolve_deck_ref(&spec.p1).map_err(|e| format!("{id} p1: {e}"))?,
        )
    } else {
        let (a, b) = id
            .split_once("-vs-")
            .ok_or_else(|| format!("unknown matchup '{id}'"))?;
        let get = |n: &str| {
            decks
                .get(n)
                .map(|d| expand_deck(d))
                .ok_or_else(|| format!("matchup '{id}': deck '{n}' not in --decks"))
        };
        (get(a)?, get(b)?)
    };
    let list = DeckList {
        player: PlayerDeckList {
            main_deck: p0,
            ..Default::default()
        },
        opponent: PlayerDeckList {
            main_deck: p1,
            ..Default::default()
        },
        ..Default::default()
    };
    Ok(resolve_deck_list(db, &list))
}

#[derive(Default, Clone, Copy)]
struct Profile {
    creatures: [u32; 2],
    lands: [u32; 2],
}

fn snapshot(state: &GameState) -> Profile {
    let mut p = Profile::default();
    for id in state.battlefield.iter() {
        let Some(obj) = state.objects.get(id) else {
            continue;
        };
        let seat = obj.controller.0 as usize;
        if seat > 1 {
            continue;
        }
        let types = &obj.card_types.core_types;
        if types.contains(&CoreType::Creature) {
            p.creatures[seat] += 1;
        }
        if types.contains(&CoreType::Land) {
            p.lands[seat] += 1;
        }
    }
    p
}

struct GameOut {
    winner: Option<PlayerId>,
    turns: u32,
    /// Board at the last batch boundary where both players were still alive —
    /// the end-of-game board reads as zero for whoever just died.
    last_alive: Profile,
    panicked: bool,
    /// Blunders per seat (`--audit` only; empty otherwise).
    audit: AuditTally,
}

/// The seat that owns the decision `state` is waiting on (the AI actor
/// `run_ai_actions` will pick for it), or `None` when nobody can act.
fn decision_seat(state: &GameState) -> Option<PlayerId> {
    state
        .waiting_for
        .acting_players()
        .into_iter()
        .next()
        .map(|p| turn_control::authorized_submitter_for_player(state, p))
}

fn run_game(
    payload: &DeckPayload,
    db: &CardDatabase,
    seed: u64,
    p0: &AiConfig,
    p1: &AiConfig,
    audit: bool,
) -> GameOut {
    let mut state = GameState::new_two_player(seed);
    load_and_hydrate_decks(&mut state, payload, Some(db));
    let _ = start_game_skip_mulligan(&mut state);
    let players: HashSet<PlayerId> = [PlayerId(0), PlayerId(1)].into_iter().collect();
    let configs: HashMap<PlayerId, AiConfig> = [
        (PlayerId(0), p0.clone().into_measurement(seed)),
        (
            PlayerId(1),
            p1.clone().into_measurement(seed.wrapping_add(1)),
        ),
    ]
    .into_iter()
    .collect();
    let mut rng = <rand::rngs::StdRng as rand::SeedableRng>::seed_from_u64(seed);
    let session = phase_ai::session::AiSession::arc_from_game(&state);
    let mut last_alive = snapshot(&state);
    let mut tally = AuditTally {
        games: 1,
        ..AuditTally::default()
    };
    // One decision per step under `--audit`, so every action can be judged
    // against the exact state it was taken in; the unbounded batch otherwise.
    let mut steps = 0usize;
    loop {
        if let WaitingFor::GameOver { winner } = &state.waiting_for {
            return GameOut {
                winner: *winner,
                turns: state.turn_number,
                last_alive,
                panicked: false,
                audit: tally,
            };
        }
        if state.turn_number >= MAX_TURNS || steps > 20_000 {
            return GameOut {
                winner: None,
                turns: state.turn_number,
                last_alive,
                panicked: false,
                audit: tally,
            };
        }
        steps += 1;
        let pre = audit.then(|| state.clone());
        let step = std::panic::catch_unwind(AssertUnwindSafe(|| {
            if audit {
                run_ai_actions_bounded(&mut state, &players, &configs, &mut rng, &session, 1)
            } else {
                run_ai_actions(&mut state, &players, &configs, &mut rng, &session)
            }
        }));
        match step {
            Ok(results) if results.is_empty() => {
                return GameOut {
                    winner: None,
                    turns: state.turn_number,
                    last_alive,
                    panicked: false,
                    audit: tally,
                }
            }
            Ok(results) => {
                if let Some(pre) = pre.as_ref() {
                    // Item i was decided in the state item i-1 left behind.
                    let mut before: &GameState = pre;
                    for item in results.results.iter() {
                        if let Some(seat) = decision_seat(before) {
                            audit_step(before, &item.action, seat, &mut tally);
                        }
                        before = &item.state;
                    }
                }
                if !matches!(state.waiting_for, WaitingFor::GameOver { .. }) {
                    last_alive = snapshot(&state);
                }
            }
            Err(_) => {
                return GameOut {
                    winner: None,
                    turns: state.turn_number,
                    last_alive,
                    panicked: true,
                    audit: tally,
                }
            }
        }
    }
}

/// `--probe N`: play N games with side B in both seats, and at every main-phase
/// priority of seat 0 with an empty stack where a creature spell is castable,
/// print how side A and side B would score "cast the creature" vs "pass".
/// This is how to find WHICH layer flips a decision, instead of guessing.
fn run_probe(
    payload: &DeckPayload,
    db: &CardDatabase,
    seed: u64,
    games: usize,
    a: &Side,
    b: &Side,
) {
    use engine::types::actions::GameAction;
    use engine::types::phase::Phase;
    let mut agree = 0usize;
    let mut a_casts = 0usize;
    let mut b_casts = 0usize;
    let mut spots = 0usize;
    for g in 0..games {
        let game_seed = seed.wrapping_add(g as u64);
        let mut state = GameState::new_two_player(game_seed);
        load_and_hydrate_decks(&mut state, payload, Some(db));
        let _ = start_game_skip_mulligan(&mut state);
        let players: HashSet<PlayerId> = [PlayerId(0), PlayerId(1)].into_iter().collect();
        let configs: HashMap<PlayerId, AiConfig> = [
            (PlayerId(0), b.config.clone().into_measurement(game_seed)),
            (
                PlayerId(1),
                b.config.clone().into_measurement(game_seed.wrapping_add(1)),
            ),
        ]
        .into_iter()
        .collect();
        let mut rng = <rand::rngs::StdRng as rand::SeedableRng>::seed_from_u64(game_seed);
        let session = phase_ai::session::AiSession::arc_from_game(&state);
        let mut guard = 0;
        while guard < 4000
            && !matches!(state.waiting_for, WaitingFor::GameOver { .. })
            && state.turn_number < MAX_TURNS
        {
            guard += 1;
            let main = matches!(state.phase, Phase::PreCombatMain | Phase::PostCombatMain);
            if main
                && state.stack.is_empty()
                && matches!(state.waiting_for, WaitingFor::Priority { player } if player == PlayerId(0))
            {
                let score = |cfg: &AiConfig| {
                    phase_ai::search::score_candidates_for_parallel_worker(
                        &state,
                        PlayerId(0),
                        &cfg.clone().into_measurement(game_seed),
                        Some(&session),
                    )
                };
                let sa = score(&a.config);
                let creature_cast = |act: &GameAction| match act {
                    GameAction::CastSpell { object_id, .. } => state
                        .objects
                        .get(object_id)
                        .is_some_and(|o| o.card_types.core_types.contains(&CoreType::Creature)),
                    _ => false,
                };
                if sa.iter().any(|(act, _)| creature_cast(act)) {
                    let sb = score(&b.config);
                    let best = |v: &[(GameAction, f64)]| {
                        v.iter().cloned().max_by(|x, y| x.1.total_cmp(&y.1))
                    };
                    let pick = |v: &[(GameAction, f64)], f: &dyn Fn(&GameAction) -> bool| {
                        v.iter()
                            .filter(|(x, _)| f(x))
                            .map(|(_, s)| *s)
                            .fold(f64::NEG_INFINITY, f64::max)
                    };
                    let is_pass = |x: &GameAction| matches!(x, GameAction::PassPriority);
                    let (ca, pa) = (pick(&sa, &creature_cast), pick(&sa, &is_pass));
                    let (cb, pb) = (pick(&sb, &creature_cast), pick(&sb, &is_pass));
                    let a_top = best(&sa).map(|x| creature_cast(&x.0)).unwrap_or(false);
                    let b_top = best(&sb).map(|x| creature_cast(&x.0)).unwrap_or(false);
                    spots += 1;
                    a_casts += a_top as usize;
                    b_casts += b_top as usize;
                    agree += (a_top == b_top) as usize;
                    let name = sa
                        .iter()
                        .find(|(x, _)| creature_cast(x))
                        .and_then(|(x, _)| match x {
                            GameAction::CastSpell { object_id, .. } => {
                                state.objects.get(object_id).map(|o| o.name.clone())
                            }
                            _ => None,
                        })
                        .unwrap_or_default();
                    if std::env::var("LAB_EXPLAIN").is_ok() && !a_top {
                        let rows = phase_ai::search::lab_explain_root(
                            &state,
                            PlayerId(0),
                            &a.config.clone().into_measurement(game_seed),
                            &session,
                        );
                        for (act, desc) in rows {
                            if creature_cast(&act) || is_pass(&act) {
                                println!(
                                    "    {:<5} {desc}",
                                    if is_pass(&act) { "PASS" } else { "CAST" }
                                );
                            }
                        }
                    }
                    println!(
                        "g{g} t{} {:?} {name:<22} A cast {ca:>8.3} pass {pa:>8.3} top={} | B cast {cb:>8.3} pass {pb:>8.3} top={}",
                        state.turn_number, state.phase, if a_top {"CAST"} else {"other"}, if b_top {"CAST"} else {"other"}
                    );
                }
            }
            let r = std::panic::catch_unwind(AssertUnwindSafe(|| {
                phase_ai::auto_play::run_ai_actions_bounded(
                    &mut state, &players, &configs, &mut rng, &session, 1,
                )
            }));
            match r {
                Ok(res) if res.is_empty() => break,
                Ok(_) => {}
                Err(_) => break,
            }
        }
    }
    println!("spots {spots}: A tops a creature cast {a_casts}, B {b_casts}, agree {agree}");
}

/// Two-sided exact sign test (binomial, p = 0.5) on decided games.
fn sign_test(a: usize, b: usize) -> f64 {
    let n = a + b;
    if n == 0 {
        return 1.0;
    }
    let k = a.min(b);
    // P(X <= k) with X ~ Bin(n, 0.5), computed in log space.
    let ln_half_n = -(n as f64) * std::f64::consts::LN_2;
    let mut ln_c = 0.0f64; // ln C(n, 0)
    let mut tail = 0.0f64;
    for i in 0..=k {
        if i > 0 {
            ln_c += ((n - i + 1) as f64).ln() - (i as f64).ln();
        }
        tail += (ln_c + ln_half_n).exp();
    }
    (2.0 * tail).min(1.0)
}

#[derive(Default)]
struct Tally {
    a: usize,
    b: usize,
    draws: usize,
    panics: usize,
    turns: u64,
    a_creatures: u64,
    b_creatures: u64,
    a_lands: u64,
    b_lands: u64,
    games: usize,
    /// Blunder counts for side A / side B (seat-corrected).
    audit: [std::collections::BTreeMap<Blunder, u32>; 2],
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut data: Option<PathBuf> = None;
    let mut a_spec = "veryhard:wasm".to_string();
    let mut b_spec = "medium:wasm".to_string();
    let mut matchups = "blue-mirror".to_string();
    let mut games = 20usize;
    let mut seed = 1000u64;
    let mut decks_path: Option<PathBuf> = None;
    let mut json_out: Option<PathBuf> = None;
    let mut probe: usize = 0;
    let mut audit = false;
    let mut i = 0;
    while i < args.len() {
        let take = |i: &mut usize| -> String {
            *i += 1;
            args.get(*i).cloned().unwrap_or_else(|| {
                eprintln!("missing value for {}", args[*i - 1]);
                std::process::exit(2)
            })
        };
        match args[i].as_str() {
            "--a" => a_spec = take(&mut i),
            "--b" => b_spec = take(&mut i),
            "--matchups" => matchups = take(&mut i),
            "--games" => games = take(&mut i).parse().expect("--games"),
            "--seed" => seed = take(&mut i).parse().expect("--seed"),
            "--decks" => decks_path = Some(PathBuf::from(take(&mut i))),
            "--json" => json_out = Some(PathBuf::from(take(&mut i))),
            "--probe" => probe = take(&mut i).parse().expect("--probe"),
            "--audit" => audit = true,
            "-h" | "--help" => {
                eprintln!(
                    "{}",
                    include_str!("ai_ladder.rs")
                        .lines()
                        .take(20)
                        .collect::<Vec<_>>()
                        .join("\n")
                );
                return;
            }
            other if other.starts_with("--") => {
                eprintln!("unknown flag {other}");
                std::process::exit(2);
            }
            other => data = Some(PathBuf::from(other)),
        }
        i += 1;
    }
    let data = data.unwrap_or_else(|| PathBuf::from("client/public"));
    let card_path = if data.is_dir() {
        data.join("card-data.json")
    } else {
        data
    };
    let db = CardDatabase::from_export(&card_path).unwrap_or_else(|e| {
        eprintln!("cannot load {}: {e}", card_path.display());
        std::process::exit(1)
    });
    let decks: HashMap<String, Vec<String>> = match &decks_path {
        Some(p) => load_decks(p),
        None => HashMap::new(),
    };
    let side_a = parse_spec(&a_spec).unwrap_or_else(|e| {
        eprintln!("--a: {e}");
        std::process::exit(2)
    });
    let side_b = parse_spec(&b_spec).unwrap_or_else(|e| {
        eprintln!("--b: {e}");
        std::process::exit(2)
    });

    let ids: Vec<String> = matchups
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let payloads: Vec<(String, DeckPayload)> = ids
        .iter()
        .map(|id| {
            let p = payload_for(&db, id, &decks).unwrap_or_else(|e| {
                eprintln!("{e}");
                std::process::exit(2)
            });
            (id.clone(), p)
        })
        .collect();

    if probe > 0 {
        run_probe(&payloads[0].1, &db, seed, probe, &side_a, &side_b);
        return;
    }
    eprintln!(
        "A = {}\nB = {}\n{} games per matchup (paired seeds)",
        side_a.label, side_b.label, games
    );
    let pairs = games.div_ceil(2);
    let started = std::time::Instant::now();

    // (matchup index, seed index, a_is_p0)
    let tasks: Vec<(usize, usize, bool)> = (0..payloads.len())
        .flat_map(|m| (0..pairs).flat_map(move |s| [(m, s, true), (m, s, false)]))
        .collect();
    let results: Vec<(usize, bool, GameOut)> = tasks
        .par_iter()
        .map(|&(m, s, a_is_p0)| {
            let game_seed = seed.wrapping_add(m as u64 * 100_000).wrapping_add(s as u64);
            let (p0, p1) = if a_is_p0 {
                (&side_a.config, &side_b.config)
            } else {
                (&side_b.config, &side_a.config)
            };
            (
                m,
                a_is_p0,
                run_game(&payloads[m].1, &db, game_seed, p0, p1, audit),
            )
        })
        .collect();

    let mut tallies: Vec<Tally> = (0..payloads.len()).map(|_| Tally::default()).collect();
    for (m, a_is_p0, out) in &results {
        let t = &mut tallies[*m];
        let (a_seat, b_seat) = if *a_is_p0 { (0, 1) } else { (1, 0) };
        t.games += 1;
        t.turns += out.turns as u64;
        t.a_creatures += out.last_alive.creatures[a_seat] as u64;
        t.b_creatures += out.last_alive.creatures[b_seat] as u64;
        t.a_lands += out.last_alive.lands[a_seat] as u64;
        t.b_lands += out.last_alive.lands[b_seat] as u64;
        if out.panicked {
            t.panics += 1;
        }
        for (side, seat) in [(0usize, a_seat), (1usize, b_seat)] {
            for (b, n) in &out.audit.per_seat[seat] {
                *t.audit[side].entry(*b).or_insert(0) += n;
            }
        }
        match out.winner {
            Some(PlayerId(w)) if w as usize == a_seat => t.a += 1,
            Some(_) => t.b += 1,
            None => t.draws += 1,
        }
    }

    println!("| matchup | A | B | draws | A% | p | turns | creat A×B | lands A×B |");
    println!("|---|---|---|---|---|---|---|---|---|");
    let mut total = Tally::default();
    let mut rows = Vec::new();
    for ((id, _), t) in payloads.iter().zip(&tallies) {
        let decided = (t.a + t.b).max(1);
        let g = t.games.max(1) as f64;
        println!(
            "| {id} | {} | {} | {} | {:.1}% | {:.3} | {:.1} | {:.1} × {:.1} | {:.1} × {:.1} |",
            t.a,
            t.b,
            t.draws,
            100.0 * t.a as f64 / decided as f64,
            sign_test(t.a, t.b),
            t.turns as f64 / g,
            t.a_creatures as f64 / g,
            t.b_creatures as f64 / g,
            t.a_lands as f64 / g,
            t.b_lands as f64 / g,
        );
        rows.push(serde_json::json!({
            "matchup": id, "a": t.a, "b": t.b, "draws": t.draws, "panics": t.panics,
            "p": sign_test(t.a, t.b),
            "avg_turns": t.turns as f64 / g,
            "creatures": [t.a_creatures as f64 / g, t.b_creatures as f64 / g],
        }));
        total.a += t.a;
        total.b += t.b;
        total.draws += t.draws;
        total.panics += t.panics;
        total.games += t.games;
        for side in 0..2 {
            for (b, n) in &t.audit[side] {
                *total.audit[side].entry(*b).or_insert(0) += n;
            }
        }
    }
    let decided = (total.a + total.b).max(1);
    println!(
        "| **total** | {} | {} | {} | {:.1}% | {:.3} | | | |",
        total.a,
        total.b,
        total.draws,
        100.0 * total.a as f64 / decided as f64,
        sign_test(total.a, total.b)
    );
    let mut audit_json = serde_json::Map::new();
    if audit {
        let g = total.games.max(1) as f64;
        println!();
        println!("| blunder | A/game | B/game |");
        println!("|---|---|---|");
        for b in Blunder::all() {
            let a_n = total.audit[0].get(b).copied().unwrap_or(0);
            let b_n = total.audit[1].get(b).copied().unwrap_or(0);
            println!(
                "| {} | {:.2} | {:.2} |",
                b.label(),
                a_n as f64 / g,
                b_n as f64 / g
            );
            audit_json.insert(
                format!("{b:?}"),
                serde_json::json!({"a": a_n, "b": b_n, "a_per_game": a_n as f64 / g, "b_per_game": b_n as f64 / g}),
            );
        }
        println!("| games audited | {} | {} |", total.games, total.games);
    }
    let [evals, credits, blocks] = phase_ai::lab_counters::snapshot();
    eprintln!("lab counters: leaf evals (stack-credit side) {evals}, stack credits {credits}, counter-gate blocks {blocks}");
    eprintln!(
        "{} games in {:.1}s ({} panics)",
        results.len(),
        started.elapsed().as_secs_f64(),
        total.panics
    );
    if let Some(path) = json_out {
        let doc = serde_json::json!({
            "a": side_a.label, "b": side_b.label, "games_per_matchup": pairs * 2, "seed": seed,
            "rows": rows,
            "total": {"a": total.a, "b": total.b, "draws": total.draws, "p": sign_test(total.a, total.b)},
            "audit": if audit { serde_json::Value::Object(audit_json) } else { serde_json::Value::Null },
        });
        std::fs::write(&path, serde_json::to_string_pretty(&doc).unwrap()).expect("write --json");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_test_is_symmetric_and_bounded() {
        assert!((sign_test(10, 10) - 1.0).abs() < 1e-9);
        assert!((sign_test(3, 7) - sign_test(7, 3)).abs() < 1e-12);
        // 1 x 39: overwhelming.
        assert!(sign_test(1, 39) < 1e-9);
        // 10 x 30 ≈ 0.0022 (the kitchen × fury number in the MagicFinder notes).
        let p = sign_test(10, 30);
        assert!(p > 0.001 && p < 0.003, "{p}");
    }

    #[test]
    fn snapshot_decks_become_count_lines() {
        let lines = deck_lines_from_snapshot(
            r#"{"name":"X","format":"modern","main":[{"name":"Island","count":4},{"name":"Ponder"}]}"#,
        )
        .expect("snapshot shape");
        assert_eq!(lines, vec!["4 Island".to_string(), "1 Ponder".to_string()]);
        assert_eq!(expand_deck(&lines).len(), 5);
        assert!(deck_lines_from_snapshot(r#"{"lab-blue": ["4 Island"]}"#).is_none());
    }

    #[test]
    fn expand_deck_reads_counts() {
        let d = expand_deck(&["4 Island".into(), "Ponder".into(), "2 Man-o'-War".into()]);
        assert_eq!(d.len(), 7);
        assert_eq!(d.iter().filter(|n| *n == "Island").count(), 4);
    }
}
