//! Combat that simulates (MagicFinder, 2026-10-26): the root attack/block
//! declaration is replayed against alternatives through the reducer.
//!
//! Each fixture is a combat the per-creature heuristic gets wrong and the
//! replay gets right; `heuristic(..)` documents the old answer so a fixture
//! that stops separating the two is noticed instead of passing for free.

use engine::game::combat::AttackTarget;
use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::types::actions::GameAction;
use engine::types::game_state::WaitingFor;
use engine::types::identifiers::ObjectId;
use engine::types::phase::Phase;
use phase_ai::choose_action;
use phase_ai::config::{create_config, AiDifficulty, Platform};
use rand::rngs::SmallRng;
use rand::SeedableRng;

fn choose(state: &engine::types::game_state::GameState, simulate: bool) -> GameAction {
    let mut config = create_config(AiDifficulty::VeryHard, Platform::Wasm);
    config.combat_simulation = simulate;
    config = config.into_measurement(7);
    let mut rng = SmallRng::seed_from_u64(7);
    choose_action(state, P0, &config, &mut rng).expect("AI should act")
}

fn attackers_of(action: &GameAction) -> Vec<ObjectId> {
    match action {
        GameAction::DeclareAttackers { attacks, .. } => {
            let mut ids: Vec<_> = attacks.iter().map(|(id, _)| *id).collect();
            ids.sort_unstable();
            ids
        }
        other => panic!("expected DeclareAttackers, got {other:?}"),
    }
}

fn blocks_of(action: &GameAction) -> Vec<(ObjectId, ObjectId)> {
    match action {
        GameAction::DeclareBlockers { assignments } => {
            let mut a = assignments.clone();
            a.sort_unstable();
            a
        }
        other => panic!("expected DeclareBlockers, got {other:?}"),
    }
}

/// P0 is at its own declare-attackers step.
fn at_attack(runner: &mut GameRunner) {
    runner.advance_to_combat();
    assert!(
        matches!(runner.state().waiting_for, WaitingFor::DeclareAttackers { player, .. } if player == P0),
        "fixture must reach P0's attack declaration, got {:?}",
        runner.state().waiting_for
    );
}

/// P1 attacks with `attackers`; P0 must block.
fn at_block(runner: &mut GameRunner, attackers: &[ObjectId]) {
    {
        let state = runner.state_mut();
        state.active_player = P1;
        state.priority_player = P1;
        state.waiting_for = WaitingFor::Priority { player: P1 };
    }
    runner.advance_to_combat();
    runner
        .act(GameAction::DeclareAttackers {
            attacks: attackers
                .iter()
                .map(|&id| (id, AttackTarget::Player(P0)))
                .collect(),
            bands: vec![],
        })
        .expect("P1 declares");
    for _ in 0..8 {
        if matches!(runner.state().waiting_for, WaitingFor::DeclareBlockers { player, .. } if player == P0)
        {
            return;
        }
        runner
            .act(GameAction::PassPriority)
            .expect("pass to blocks");
    }
    panic!(
        "fixture must reach P0's block declaration, got {:?}",
        runner.state().waiting_for
    );
}

#[test]
fn double_block_kills_the_attacker_the_heuristic_let_through() {
    let mut s = GameScenario::new();
    s.at_phase(Phase::PreCombatMain);
    let bear = s.add_creature(P0, "Bear", 2, 2).id();
    let ogre = s.add_creature(P0, "Ogre", 3, 3).id();
    let brute = s.add_creature(P1, "Brute", 4, 4).id();
    let mut r = s.build();
    at_block(&mut r, &[brute]);

    assert_eq!(
        blocks_of(&choose(r.state(), false)),
        vec![],
        "heuristic: no block"
    );
    let mut want = vec![(bear, brute), (ogre, brute)];
    want.sort_unstable();
    assert_eq!(
        blocks_of(&choose(r.state(), true)),
        want,
        "2 + 3 damage kills the 4/4 for one of ours"
    );
}

#[test]
fn chump_with_the_token_and_let_the_angel_eat_the_wolf() {
    let mut s = GameScenario::new();
    s.at_phase(Phase::PreCombatMain);
    s.with_life(P0, 5);
    let token = s.add_creature(P0, "Token", 1, 1).id();
    let angel = s.add_creature(P0, "Angel", 4, 4).flying().id();
    let rhino = s.add_creature(P1, "Rhino", 5, 5).id();
    let wolf = s.add_creature(P1, "Wolf", 2, 2).id();
    let mut r = s.build();
    at_block(&mut r, &[rhino, wolf]);

    let mut old = vec![(angel, rhino), (token, wolf)];
    old.sort_unstable();
    assert_eq!(
        blocks_of(&choose(r.state(), false)),
        old,
        "heuristic: the 4/4 dies to the 5/5 and the wolf lives"
    );
    let mut want = vec![(token, rhino), (angel, wolf)];
    want.sort_unstable();
    assert_eq!(blocks_of(&choose(r.state(), true)), want);
}

#[test]
fn keeps_the_deathtouch_blocker_home_against_a_fatty() {
    let mut s = GameScenario::new();
    s.at_phase(Phase::PreCombatMain);
    let snake = s.add_creature(P0, "Snake", 1, 1).deathtouch().id();
    s.add_creature(P1, "Fatty", 6, 6);
    let mut r = s.build();
    at_attack(&mut r);

    assert_eq!(
        attackers_of(&choose(r.state(), false)),
        vec![snake],
        "heuristic: pokes for 1"
    );
    assert_eq!(
        attackers_of(&choose(r.state(), true)),
        Vec::<ObjectId>::new(),
        "tapped, the snake can't stop a 6-point swing back"
    );
}

#[test]
fn first_strike_attack_is_kept() {
    let mut s = GameScenario::new();
    s.at_phase(Phase::PreCombatMain);
    let knight = s.add_creature(P0, "Knight", 3, 2).first_strike().id();
    s.add_creature(P1, "Bear", 3, 3);
    let mut r = s.build();
    at_attack(&mut r);
    assert_eq!(attackers_of(&choose(r.state(), true)), vec![knight]);
}

#[test]
fn lethal_alpha_is_never_second_guessed() {
    let mut s = GameScenario::new();
    s.at_phase(Phase::PreCombatMain);
    s.with_life(P1, 4);
    let a = s.add_creature(P0, "A", 2, 2).id();
    let b = s.add_creature(P0, "B", 2, 2).id();
    let c = s.add_creature(P0, "C", 2, 2).id();
    s.add_creature(P1, "Wall", 0, 5);
    let mut r = s.build();
    at_attack(&mut r);
    let mut all = vec![a, b, c];
    all.sort_unstable();
    assert_eq!(attackers_of(&choose(r.state(), true)), all);
}

/// Replays the "jogada ruim do robô" positions the MagicFinder table recorded
/// (`npm run bot:flags -- --out <dir>` in the magic repo) through the root
/// combat decision with the replay off and on. Not a gate — the positions live
/// outside the repo — but the honest check that the replay answers the combat
/// complaints people actually sent.
///
///   FLAG_DIR=<dir> cargo test -p phase-ai --test combat_sim flag_positions -- --ignored --nocapture
#[test]
#[ignore]
fn flag_positions() {
    use engine::types::game_state::PersistedGameState;
    use engine::types::player::PlayerId;
    let dir = std::env::var("FLAG_DIR").expect("FLAG_DIR");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .expect("dir")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("json"))
        .collect();
    files.sort();
    for path in files {
        let raw: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let wait = raw["wait_type"].as_str().unwrap_or("");
        if wait != "DeclareAttackers" && wait != "DeclareBlockers" {
            continue;
        }
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let seat = PlayerId(raw["seat"].as_u64().unwrap_or(1) as u8);
        let state = match serde_json::from_value::<PersistedGameState>(raw["state"].clone())
            .map_err(|e| e.to_string())
            .and_then(|p| p.into_game_state().map_err(|e| format!("{e:?}")))
        {
            Ok(state) => state,
            Err(e) => {
                println!("{name}: does not restore ({})", &e[..e.len().min(120)]);
                continue;
            }
        };
        let pick = |simulate: bool| {
            let mut config = create_config(AiDifficulty::VeryHard, Platform::Wasm);
            config.combat_simulation = simulate;
            config.search.determinization_samples = 4;
            let config = config.into_measurement(7);
            let mut rng = SmallRng::seed_from_u64(7);
            choose_action(&state, seat, &config, &mut rng)
        };
        let names = |action: &Option<GameAction>| -> String {
            let n = |id: &ObjectId| {
                state
                    .objects
                    .get(id)
                    .map(|o| o.name.clone())
                    .unwrap_or_else(|| format!("#{}", id.0))
            };
            match action {
                Some(GameAction::DeclareAttackers { attacks, .. }) => {
                    let v: Vec<String> = attacks.iter().map(|(id, _)| n(id)).collect();
                    format!("attack [{}]", v.join(", "))
                }
                Some(GameAction::DeclareBlockers { assignments }) => {
                    let v: Vec<String> = assignments
                        .iter()
                        .map(|(b, a)| format!("{} → {}", n(b), n(a)))
                        .collect();
                    format!("block [{}]", v.join(", "))
                }
                other => format!("{other:?}"),
            }
        };
        let (old, new) = (pick(false), pick(true));
        let me = &state.players[seat.0 as usize];
        let opp = &state.players[1 - seat.0 as usize];
        println!(
            "{name} T{} life {}×{} {}\n  note: {}\n  before: {}\n  after:  {}",
            state.turn_number,
            me.life,
            opp.life,
            if old == new { "(same)" } else { "CHANGED" },
            raw["note"]
                .as_str()
                .unwrap_or("")
                .chars()
                .take(150)
                .collect::<String>(),
            names(&old),
            names(&new),
        );
    }
}
