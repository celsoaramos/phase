//! Turn that simulates (MagicFinder, 2026-10-27) — phase 2 of "a bot that is
//! strong against people".
//!
//! The root search walks MICRO-actions (pass, target, pay) two plies deep in the
//! browser: from a main-phase decision it never reaches the combat, the end of
//! the turn or the opponent's reply. A person beats it by the line, not by the
//! single play — the land kept back for the instant, the creature that would
//! have blocked the swing, the removal spent on the wrong body.
//!
//! This layer looks a whole turn ahead at the decisions that matter: a
//! main-phase priority window of the AI's own turn with an empty stack. It
//! takes the handful of best-scored root actions and plays each one out through
//! the real reducer — the rest of the AI's turn and the opponent's whole turn —
//! with the cheap heuristic AI answering every prompt for both seats, and scores
//! the position when the AI's next turn begins.
//!
//! - The opponent's hidden cards are resampled per world when the config plays
//!   fair (`determinization_samples > 0`, what the browser serves), and every
//!   line is played in the SAME worlds with the same seeds: the comparison is
//!   paired, so a lucky draw cannot crown a line.
//! - The searched answer (the highest root score) is the incumbent. Another line
//!   replaces it only when it wins on average by `SWITCH_MARGIN` and is not
//!   worse in any world by more than a trade; anything the playout cannot finish
//!   drops that line, and an unmeasurable incumbent leaves the root untouched.

use std::cmp::Ordering;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::sync::Arc;

use engine::game::engine::apply_interaction_for_simulation;
use engine::game::players;
use engine::game::turn_control;
use engine::types::actions::GameAction;
use engine::types::game_state::{GameState, WaitingFor};
use engine::types::phase::Phase;
use engine::types::player::PlayerId;
use engine::util::Deadline;
use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;

use crate::config::AiConfig;
use crate::eval::{evaluate_state, EvalWeights};
use crate::session::AiSession;

/// Root actions compared, the incumbent included (pass is always one of them).
const MAX_LINES: usize = 5;
/// Opponent worlds each line is played in.
const WORLDS: u64 = 4;
/// Engine dispatches one playout may take before it is dropped. A full turn of
/// each side is typically 40–120.
const MAX_STEPS: usize = 400;
/// An alternative must beat the incumbent by this much on average (evaluation
/// units: a point of life ~0.5, a 2/2 ~6). Below it the search's own answer
/// stands — two heuristic playouts are not precise enough to overrule it on a
/// coin flip.
pub(crate) const SWITCH_MARGIN: f64 = 2.0;
/// ...and may not lose to it in any single world by more than this (a bad trade).
const WORST_WORLD_SLACK: f64 = 6.0;
/// `evaluate_state` returns ±10_000 for a decided game.
const DECIDED: f64 = 9_000.0;

/// Process-wide counters for the lab (`ai-ladder` prints them): decisions
/// measured, lines swapped, and the time spent.
static DECISIONS: AtomicU64 = AtomicU64::new(0);
static SWITCHES: AtomicU64 = AtomicU64::new(0);
static TOTAL_MICROS: AtomicU64 = AtomicU64::new(0);
static MAX_MICROS: AtomicU64 = AtomicU64::new(0);
static PLAYOUTS: AtomicU64 = AtomicU64::new(0);
static PLAYOUTS_DROPPED: AtomicU64 = AtomicU64::new(0);
static PLAYOUT_STEPS: AtomicU64 = AtomicU64::new(0);

/// `(playouts run, playouts that could not finish, engine steps taken)`.
pub fn playout_stats() -> (u64, u64, u64) {
    (
        PLAYOUTS.load(AtomicOrdering::Relaxed),
        PLAYOUTS_DROPPED.load(AtomicOrdering::Relaxed),
        PLAYOUT_STEPS.load(AtomicOrdering::Relaxed),
    )
}

/// `(decisions measured, lines swapped, total µs, slowest decision µs)`.
pub fn stats() -> (u64, u64, u64, u64) {
    (
        DECISIONS.load(AtomicOrdering::Relaxed),
        SWITCHES.load(AtomicOrdering::Relaxed),
        TOTAL_MICROS.load(AtomicOrdering::Relaxed),
        MAX_MICROS.load(AtomicOrdering::Relaxed),
    )
}

pub(crate) struct TurnSimInput<'a> {
    pub state: &'a GameState,
    pub player: PlayerId,
    pub config: &'a AiConfig,
    pub session: &'a Arc<AiSession>,
    pub weights: &'a EvalWeights,
    /// Wall-clock guard for interactive play; `None` in measurement mode, where
    /// only the step and line caps bound the work.
    pub deadline: Option<Deadline>,
}

/// Whether `state` is a decision this layer looks a turn ahead for.
pub(crate) fn applies(state: &GameState, player: PlayerId) -> bool {
    if state.players.len() != 2 || state.format_config.team_based {
        return false;
    }
    if players::opponents(state, player).len() != 1 {
        return false;
    }
    matches!(state.waiting_for, WaitingFor::Priority { player: p } if p == player)
        && state.active_player == player
        && state.stack.is_empty()
        && matches!(state.phase, Phase::PreCombatMain | Phase::PostCombatMain)
}

fn by_score_desc(a: &(GameAction, f64), b: &(GameAction, f64)) -> Ordering {
    b.1.partial_cmp(&a.1)
        .unwrap_or(Ordering::Equal)
        .then_with(|| a.0.cmp_stable(&b.0))
}

/// The lines to compare, incumbent first: the searched argmax, then the next
/// best-scored root actions that are not a bare mana activation, then passing.
fn lines(scored: &[(GameAction, f64)], is_mana: impl Fn(&GameAction) -> bool) -> Vec<GameAction> {
    let mut ranked: Vec<&(GameAction, f64)> = scored.iter().collect();
    ranked.sort_by(|a, b| by_score_desc(a, b));
    let Some(((incumbent, _), rest)) = ranked.split_first() else {
        return Vec::new();
    };
    let mut out = vec![incumbent.clone()];
    let pass = rest
        .iter()
        .find(|(action, _)| matches!(action, GameAction::PassPriority))
        .map(|(action, _)| action.clone());
    let room = MAX_LINES - usize::from(pass.is_some());
    for (action, _) in rest {
        if out.len() >= room {
            break;
        }
        if matches!(action, GameAction::PassPriority) || is_mana(action) {
            continue;
        }
        out.push(action.clone());
    }
    out.extend(pass);
    out
}

fn horizon(state: &GameState, player: PlayerId, turn: u32) -> bool {
    matches!(state.waiting_for, WaitingFor::GameOver { .. })
        || (state.turn_number > turn && state.active_player == player)
}

/// The cheap heuristic AI that answers every prompt inside a playout.
fn playout_policy(config: &AiConfig, seed: u64) -> AiConfig {
    let mut policy = config.clone();
    policy.search.enabled = false;
    policy.search.determinization_samples = 0;
    policy.combat_simulation = false;
    policy.turn_simulation = false;
    policy.into_measurement(seed)
}

/// Play `first` from `world`, then both seats with `policy`, until the AI's next
/// turn begins. `None` when the playout cannot be finished.
fn playout(
    world: &GameState,
    player: PlayerId,
    first: &GameAction,
    policy: &AiConfig,
    session: &Arc<AiSession>,
    seed: u64,
    deadline: Option<Deadline>,
) -> Option<GameState> {
    PLAYOUTS.fetch_add(1, AtomicOrdering::Relaxed);
    let end = playout_inner(world, player, first, policy, session, seed, deadline);
    if end.is_none() {
        PLAYOUTS_DROPPED.fetch_add(1, AtomicOrdering::Relaxed);
    }
    end
}

fn playout_inner(
    world: &GameState,
    player: PlayerId,
    first: &GameAction,
    policy: &AiConfig,
    session: &Arc<AiSession>,
    seed: u64,
    deadline: Option<Deadline>,
) -> Option<GameState> {
    let turn = world.turn_number;
    let mut state = world.clone();
    let actor = turn_control::authorized_submitter_for_player(&state, player);
    apply_interaction_for_simulation(&mut state, actor, player, first.clone()).ok()?;
    let mut rng = ChaCha20Rng::seed_from_u64(seed);
    for _ in 0..MAX_STEPS {
        if horizon(&state, player, turn) {
            return Some(state);
        }
        if deadline.is_some_and(|d| d.expired()) {
            return None;
        }
        let owner = state
            .waiting_for
            .acting_player()
            .or_else(|| state.waiting_for.acting_players().first().copied())?;
        PLAYOUT_STEPS.fetch_add(1, AtomicOrdering::Relaxed);
        let action =
            crate::search::choose_action_for_playout(&state, owner, policy, &mut rng, session)?;
        let actor = turn_control::authorized_submitter_for_player(&state, owner);
        apply_interaction_for_simulation(&mut state, actor, owner, action).ok()?;
    }
    None
}

/// Measure the best root lines a whole turn ahead and return the scores the
/// root should choose from: unchanged, or the one line that clearly wins.
pub(crate) fn refine_root_turn(
    input: &TurnSimInput<'_>,
    scored: Vec<(GameAction, f64)>,
    is_mana: impl Fn(&GameAction) -> bool,
) -> Vec<(GameAction, f64)> {
    if scored.len() < 2 || !applies(input.state, input.player) {
        return scored;
    }
    let started = web_time::Instant::now();
    let before = scored.len();
    let out = measure_lines(input, scored, is_mana);
    let micros = started.elapsed().as_micros() as u64;
    DECISIONS.fetch_add(1, AtomicOrdering::Relaxed);
    if out.len() == 1 && before > 1 {
        SWITCHES.fetch_add(1, AtomicOrdering::Relaxed);
    }
    TOTAL_MICROS.fetch_add(micros, AtomicOrdering::Relaxed);
    MAX_MICROS.fetch_max(micros, AtomicOrdering::Relaxed);
    out
}

fn measure_lines(
    input: &TurnSimInput<'_>,
    scored: Vec<(GameAction, f64)>,
    is_mana: impl Fn(&GameAction) -> bool,
) -> Vec<(GameAction, f64)> {
    let lines = lines(&scored, is_mana);
    if lines.len() < 2 {
        return scored;
    }

    let base_seed = crate::planner::quick_state_hash(input.state).wrapping_add(input.state.rng_seed);
    let worlds: Vec<(GameState, u64)> = (0..WORLDS)
        .map(|i| {
            let seed = base_seed.wrapping_add(crate::determinize::splitmix64(i));
            let world = if input.config.search.determinization_samples > 0 {
                let mut rng = ChaCha20Rng::seed_from_u64(seed);
                crate::determinize::determinize_opponents(input.state, input.player, &mut rng)
            } else {
                input.state.clone()
            };
            (world, seed)
        })
        .collect();
    let policy = playout_policy(input.config, base_seed);
    // Playouts must not write the authoritative session (Pact routes, caches
    // keyed to the real position): they get their own copy.
    let session = Arc::new((**input.session).clone());

    let mut values: Vec<Option<Vec<f64>>> = Vec::with_capacity(lines.len());
    for line in &lines {
        let mut per_world = Vec::with_capacity(worlds.len());
        for (world, seed) in &worlds {
            match playout(
                world,
                input.player,
                line,
                &policy,
                &session,
                *seed,
                input.deadline,
            ) {
                Some(end) => per_world.push(evaluate_state(&end, input.player, input.weights)),
                None => break,
            }
        }
        if per_world.len() < worlds.len() {
            if values.is_empty() {
                // The searched answer cannot be measured: leave the root alone.
                return scored;
            }
            values.push(None);
        } else {
            values.push(Some(per_world));
        }
        if input.deadline.is_some_and(|d| d.expired()) {
            break;
        }
    }

    let Some(Some(incumbent)) = values.first().cloned() else {
        return scored;
    };
    let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
    let incumbent_mean = mean(&incumbent);
    // A decided game is decided: nothing to improve on a win, and a loss in
    // every line is not a reason to swap.
    if incumbent_mean >= DECIDED {
        return scored;
    }

    let mut best: Option<(f64, usize)> = None;
    for (i, value) in values.iter().enumerate().skip(1) {
        let Some(per_world) = value else { continue };
        let worst_gap = per_world
            .iter()
            .zip(&incumbent)
            .map(|(line, inc)| line - inc)
            .fold(f64::INFINITY, f64::min);
        if worst_gap < -WORST_WORLD_SLACK {
            continue;
        }
        let m = mean(per_world);
        if best.is_none_or(|(b, _)| m > b) {
            best = Some((m, i));
        }
    }
    match best {
        Some((m, i)) if m - incumbent_mean >= SWITCH_MARGIN => vec![(lines[i].clone(), 1.0)],
        _ => scored,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::types::identifiers::ObjectId;

    #[test]
    fn lines_lead_with_the_argmax_and_always_include_pass() {
        let land = GameAction::PlayLand {
            object_id: ObjectId(5),
            card_id: engine::types::identifiers::CardId(5),
        };
        let pass = GameAction::PassPriority;
        let scored = vec![(pass.clone(), 0.1), (land.clone(), 2.0)];
        let out = lines(&scored, |_| false);
        assert_eq!(out[0], land);
        assert!(out.contains(&pass));
        assert_eq!(out.len(), 2);
    }
}
