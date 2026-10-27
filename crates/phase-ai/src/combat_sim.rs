//! Combat that simulates (MagicFinder, 2026-10-26).
//!
//! The heuristic combat AI (`combat_ai`) judges each attacker alone against the
//! best block, reads the race from `life / power`, and never runs the reducer —
//! and the root search hands it every declaration without looking further. A
//! person who plays well beats it by attacking into the AI's open board and by
//! blocking the swing it should not have made.
//!
//! This layer keeps the heuristic declaration as the INCUMBENT and compares it
//! with a handful of alternatives, each replayed through the real reducer to
//! the end of combat damage:
//!
//! - attacking, the defender answers with its own block heuristic, with no
//!   block at all and — when the declaration space is small — with every legal
//!   block; an attack is worth its WORST answer (CR 509.1: the defender chooses);
//! - blocking, every candidate block is replayed against the declared attack.
//!
//! The position after damage is scored by the same evaluation the search uses,
//! minus a survival term for the next swing the opponent can make at the board
//! the combat left behind (attackers stay tapped through the opponent's turn —
//! CR 502.3 untaps them only in their controller's untap step).
//!
//! Anything the replay cannot drive without making a choice for a player
//! (targets, damage assignment, modal triggers) drops that candidate. When the
//! incumbent itself cannot be replayed, the heuristic answer stands unchanged:
//! this layer only ever swaps a declaration it has measured for a better one.

use std::collections::HashMap;

use engine::ai_support::legal_actions;
use engine::game::combat::{
    complete_blocker_proposal, pending_combat_tax_is_affordable, validate_blockers_for_player,
    AttackTarget, CombatTaxPosture,
};
use engine::game::engine::apply_as_current_for_simulation;
use engine::game::players;
use engine::types::actions::GameAction;
use engine::types::game_state::{CombatDamageAssignmentMode, GameState, WaitingFor};
use engine::types::identifiers::ObjectId;
use engine::types::phase::Phase;
use engine::types::player::PlayerId;
use engine::util::Deadline;

use crate::combat_ai::{choose_blockers_with_profile, crackback_damage};
use crate::config::AiProfile;
use crate::eval::{evaluate_state, EvalWeights};

/// Reducer steps one replay may take before it gives up (passes, blocks, tax).
const MAX_REPLAY_STEPS: usize = 32;
/// Attack declarations compared, the incumbent included.
const MAX_ATTACK_ALTERNATIVES: usize = 12;
/// Defender answers enumerated in full per attack; above this the defender
/// answers only with its heuristic and with no block.
const MAX_DEFENDER_ENUMERATION: usize = 24;
/// Our own block declarations enumerated in full; above this, one-blocker
/// changes to the heuristic block.
const MAX_BLOCK_ENUMERATION: usize = 64;
/// Hard cap on reducer replays per decision. Bounds the browser cost and keeps
/// the choice deterministic in measurement mode (no wall clock involved).
const MAX_REPLAYS: usize = 160;
/// An alternative must beat the incumbent by this much (evaluation units: a
/// point of life is ~0.5, a 2/2 ~6) — measurement noise must not churn the
/// heuristic's choice.
pub(crate) const SWITCH_MARGIN: f64 = 1.0;
/// Penalty for a board the opponent can kill next swing (≈ four creatures:
/// well below a real loss, well above any trade).
const LETHAL_NEXT_SWING: f64 = 30.0;
/// `evaluate_state` returns ±10_000 for a decided game.
const DECIDED: f64 = 9_000.0;

/// What a replay needs from the caller.
pub(crate) struct CombatSimInput<'a> {
    pub state: &'a GameState,
    pub player: PlayerId,
    pub weights: &'a EvalWeights,
    /// Wall-clock guard for interactive play; `None` in measurement mode, where
    /// only `MAX_REPLAYS` bounds the work.
    pub deadline: Option<Deadline>,
}

struct Budget {
    replays: usize,
    deadline: Option<Deadline>,
}

impl Budget {
    fn take(&mut self) -> bool {
        if self.replays >= MAX_REPLAYS || self.deadline.is_some_and(|d| d.expired()) {
            return false;
        }
        self.replays += 1;
        true
    }
}

/// Where a replay stopped.
enum Reached {
    /// The defender must declare blocks.
    Blockers {
        defender: PlayerId,
        targets: HashMap<ObjectId, Vec<ObjectId>>,
    },
    /// Combat damage is done (or the game ended, or combat was skipped).
    Finished,
    /// A prompt the replay will not answer for a player.
    Unsupported,
}

fn opponent_of(state: &GameState, player: PlayerId) -> Option<PlayerId> {
    if state.players.len() != 2 || state.format_config.team_based {
        return None;
    }
    let opponents = players::opponents(state, player);
    (opponents.len() == 1).then(|| opponents[0])
}

fn combat_is_over(state: &GameState, turn: u32) -> bool {
    if matches!(state.waiting_for, WaitingFor::GameOver { .. }) {
        return true;
    }
    if state.turn_number != turn || state.phase >= Phase::EndCombat {
        return true;
    }
    state.stack.is_empty()
        && matches!(state.waiting_for, WaitingFor::Priority { .. })
        && state
            .combat
            .as_ref()
            .is_some_and(|combat| combat.regular_damage_done)
}

/// Pass priority (and answer a combat-tax quote) until the defender must
/// block or combat damage is done. `stop_at_blocks` = false makes a pending
/// block declaration `Unsupported` (the caller already declared).
fn advance(state: &mut GameState, turn: u32, stop_at_blocks: bool) -> Reached {
    for _ in 0..MAX_REPLAY_STEPS {
        if combat_is_over(state, turn) {
            return Reached::Finished;
        }
        let action = match &state.waiting_for {
            WaitingFor::DeclareBlockers {
                player,
                valid_block_targets,
                ..
            } if stop_at_blocks => {
                return Reached::Blockers {
                    defender: *player,
                    targets: valid_block_targets.clone(),
                };
            }
            WaitingFor::Priority { .. } => GameAction::PassPriority,
            // CR 508.1j + CR 509.1f: the quote belongs to the declaration that
            // opened it; answer it the way the root combat path does.
            WaitingFor::CombatTaxPayment { .. } => GameAction::PayCombatTax {
                accept: pending_combat_tax_is_affordable(state),
            },
            // CR 510.1c: an attacker blocked by two or more divides its damage.
            // Answer with the engine-issued Normal division, the same one the
            // AI's own fallback submits — without it every double block would
            // be unmeasurable.
            WaitingFor::AssignCombatDamage { .. } => {
                match legal_actions(state).into_iter().find(|action| {
                    matches!(
                        action,
                        GameAction::AssignCombatDamage {
                            mode: CombatDamageAssignmentMode::Normal,
                            ..
                        }
                    )
                }) {
                    Some(action) => action,
                    None => return Reached::Unsupported,
                }
            }
            _ => return Reached::Unsupported,
        };
        if apply_as_current_for_simulation(state, action).is_err() {
            return Reached::Unsupported;
        }
    }
    Reached::Unsupported
}

/// Damage the opponent's whole creature board can push through `player`'s
/// untapped creatures on its next swing. `untap_mine` = the player untaps
/// before that swing (we are blocking now; our turn comes first).
fn next_swing(state: &GameState, player: PlayerId, opponent: PlayerId, untap_mine: bool) -> i32 {
    let mut view = state.clone();
    let battlefield: Vec<ObjectId> = view.battlefield.iter().copied().collect();
    for id in battlefield {
        if let Some(object) = view.objects.get_mut(&id) {
            if object.controller == opponent || (untap_mine && object.controller == player) {
                object.tapped = false;
            }
        }
    }
    crackback_damage(&view, player, &[opponent], &[], None)
}

/// Evaluation-unit cost of facing `threat` damage at `life`. Concave: the last
/// points of life are worth more than the first.
pub(crate) fn survival_penalty(life: i32, threat: i32, life_weight: f64) -> f64 {
    if threat <= 0 || life <= 0 {
        return 0.0;
    }
    if threat >= life {
        return LETHAL_NEXT_SWING;
    }
    let margin = f64::from(life - threat);
    life_weight.max(0.25) * f64::from(threat) * (0.5 + 3.0 / margin)
}

fn position_value(
    state: &GameState,
    player: PlayerId,
    opponent: PlayerId,
    weights: &EvalWeights,
    untap_mine: bool,
) -> f64 {
    let base = evaluate_state(state, player, weights);
    if base.abs() >= DECIDED {
        return base;
    }
    let life = state.players[player.0 as usize].life;
    base - survival_penalty(
        life,
        next_swing(state, player, opponent, untap_mine),
        weights.life,
    )
}

/// Every legal block declaration for `defender`, if there are at most `cap`
/// raw combinations (each blocker: no block, or one of its legal attackers).
fn enumerate_blocks(
    state: &GameState,
    defender: PlayerId,
    targets: &HashMap<ObjectId, Vec<ObjectId>>,
    cap: usize,
) -> Option<Vec<Vec<(ObjectId, ObjectId)>>> {
    let mut blockers: Vec<ObjectId> = targets.keys().copied().collect();
    blockers.sort_unstable();
    let mut product = 1usize;
    for blocker in &blockers {
        product = product.checked_mul(targets[blocker].len() + 1)?;
        if product > cap {
            return None;
        }
    }
    let mut out: Vec<Vec<(ObjectId, ObjectId)>> = vec![Vec::new()];
    for blocker in &blockers {
        let mut attackers = targets[blocker].clone();
        attackers.sort_unstable();
        let mut next = Vec::with_capacity(out.len() * (attackers.len() + 1));
        for partial in &out {
            next.push(partial.clone());
            for &attacker in &attackers {
                let mut with = partial.clone();
                with.push((*blocker, attacker));
                next.push(with);
            }
        }
        out = next;
    }
    out.retain(|declaration| validate_blockers_for_player(state, defender, declaration).is_ok());
    Some(out)
}

fn attacker_ids_at(state: &GameState, defender: PlayerId) -> Vec<ObjectId> {
    state
        .combat
        .as_ref()
        .map(|combat| {
            combat
                .attackers
                .iter()
                .filter(|a| a.defending_player == defender)
                .map(|a| a.object_id)
                .collect()
        })
        .unwrap_or_default()
}

/// Worst-case value (for the attacker) of one complete attack declaration.
fn attack_value(
    input: &CombatSimInput<'_>,
    opponent: PlayerId,
    action: &GameAction,
    budget: &mut Budget,
) -> Option<f64> {
    let turn = input.state.turn_number;
    if !budget.take() {
        return None;
    }
    let mut declared = input.state.clone();
    apply_as_current_for_simulation(&mut declared, action.clone()).ok()?;
    let (defender, targets) = match advance(&mut declared, turn, true) {
        Reached::Finished => {
            return Some(position_value(
                &declared,
                input.player,
                opponent,
                input.weights,
                false,
            ))
        }
        Reached::Unsupported => return None,
        Reached::Blockers { defender, targets } => (defender, targets),
    };
    if defender != opponent {
        return None;
    }

    let mut answers: Vec<Vec<(ObjectId, ObjectId)>> =
        match enumerate_blocks(&declared, defender, &targets, MAX_DEFENDER_ENUMERATION) {
            Some(all) => all,
            None => {
                let attackers = attacker_ids_at(&declared, defender);
                let model = choose_blockers_with_profile(
                    &declared,
                    defender,
                    &attackers,
                    &AiProfile::default(),
                    Some(&targets),
                );
                vec![model, Vec::new()]
            }
        };
    answers.dedup();

    let mut worst: Option<f64> = None;
    for answer in answers {
        if !budget.take() {
            // A partial worst case would flatter this attack.
            return None;
        }
        let mut branch = declared.clone();
        let blocks =
            complete_blocker_proposal(&branch, defender, &answer, CombatTaxPosture::Refuse);
        if apply_as_current_for_simulation(&mut branch, blocks).is_err() {
            continue;
        }
        match advance(&mut branch, turn, false) {
            Reached::Finished => {}
            // The defender's answer led to a prompt the replay will not answer
            // (a trigger target, a damage-assignment order): this attack can't
            // be measured honestly.
            _ => return None,
        }
        let value = position_value(&branch, input.player, opponent, input.weights, false);
        worst = Some(worst.map_or(value, |w: f64| w.min(value)));
    }
    worst
}

/// The attack declarations to compare, incumbent first: the heuristic's, no
/// attack, everyone, and the heuristic with each eligible creature toggled.
fn attack_alternatives(
    incumbent: &[(ObjectId, AttackTarget)],
    eligible: &[ObjectId],
    opponent: PlayerId,
) -> Vec<Vec<(ObjectId, AttackTarget)>> {
    let target_for = |id: ObjectId| {
        incumbent
            .iter()
            .find(|(attacker, _)| *attacker == id)
            .map(|(_, target)| *target)
            .unwrap_or(AttackTarget::Player(opponent))
    };
    let mut eligible: Vec<ObjectId> = eligible.to_vec();
    eligible.sort_unstable();
    eligible.dedup();

    let mut out: Vec<Vec<(ObjectId, AttackTarget)>> = vec![incumbent.to_vec(), Vec::new()];
    out.push(eligible.iter().map(|&id| (id, target_for(id))).collect());
    for &id in &eligible {
        let mut toggled: Vec<(ObjectId, AttackTarget)> = incumbent.to_vec();
        if let Some(pos) = toggled.iter().position(|(attacker, _)| *attacker == id) {
            toggled.remove(pos);
        } else {
            toggled.push((id, target_for(id)));
        }
        out.push(toggled);
    }
    let mut seen: Vec<Vec<ObjectId>> = Vec::new();
    out.retain(|attacks| {
        let mut key: Vec<ObjectId> = attacks.iter().map(|(id, _)| *id).collect();
        key.sort_unstable();
        if seen.contains(&key) {
            false
        } else {
            seen.push(key);
            true
        }
    });
    out.truncate(MAX_ATTACK_ALTERNATIVES);
    out
}

/// Measure the heuristic attack against its alternatives and return the one to
/// play. `complete` turns a raw declaration into the engine action the root
/// would submit (combat-tax planning included).
pub(crate) fn refine_attack(
    input: &CombatSimInput<'_>,
    incumbent: GameAction,
    complete: impl Fn(Vec<(ObjectId, AttackTarget)>) -> GameAction,
) -> GameAction {
    let Some(opponent) = opponent_of(input.state, input.player) else {
        return incumbent;
    };
    let WaitingFor::DeclareAttackers {
        player,
        valid_attacker_ids,
        ..
    } = &input.state.waiting_for
    else {
        return incumbent;
    };
    if *player != input.player {
        return incumbent;
    }
    let GameAction::DeclareAttackers { attacks, .. } = &incumbent else {
        return incumbent;
    };

    let mut budget = Budget {
        replays: 0,
        deadline: input.deadline,
    };
    let Some(incumbent_value) = attack_value(input, opponent, &incumbent, &mut budget) else {
        return incumbent;
    };
    if incumbent_value >= DECIDED {
        return incumbent;
    }

    let mut best: Option<(f64, GameAction)> = None;
    for attacks in attack_alternatives(attacks, valid_attacker_ids, opponent)
        .into_iter()
        .skip(1)
    {
        let action = complete(attacks);
        if action == incumbent || best.as_ref().is_some_and(|(_, b)| *b == action) {
            continue;
        }
        let Some(value) = attack_value(input, opponent, &action, &mut budget) else {
            continue;
        };
        if best.as_ref().is_none_or(|(b, _)| value > *b) {
            best = Some((value, action));
        }
    }
    match best {
        Some((value, action)) if value - incumbent_value >= SWITCH_MARGIN => action,
        _ => incumbent,
    }
}

fn block_value(
    input: &CombatSimInput<'_>,
    opponent: PlayerId,
    action: &GameAction,
    budget: &mut Budget,
) -> Option<f64> {
    if !budget.take() {
        return None;
    }
    let turn = input.state.turn_number;
    let mut branch = input.state.clone();
    apply_as_current_for_simulation(&mut branch, action.clone()).ok()?;
    match advance(&mut branch, turn, false) {
        Reached::Finished => Some(position_value(
            &branch,
            input.player,
            opponent,
            input.weights,
            true,
        )),
        _ => None,
    }
}

/// Block declarations to compare, incumbent first: no block, then every legal
/// declaration when the space is small, else each one-blocker change to the
/// heuristic block (remove it, or move it onto another attacker it can block).
fn block_alternatives(
    state: &GameState,
    player: PlayerId,
    incumbent: &[(ObjectId, ObjectId)],
    targets: &HashMap<ObjectId, Vec<ObjectId>>,
) -> Vec<Vec<(ObjectId, ObjectId)>> {
    let mut out: Vec<Vec<(ObjectId, ObjectId)>> = vec![incumbent.to_vec(), Vec::new()];
    if let Some(all) = enumerate_blocks(state, player, targets, MAX_BLOCK_ENUMERATION) {
        out.extend(all);
    } else {
        let mut blockers: Vec<ObjectId> = targets.keys().copied().collect();
        blockers.sort_unstable();
        for blocker in blockers {
            let without: Vec<(ObjectId, ObjectId)> = incumbent
                .iter()
                .copied()
                .filter(|(b, _)| *b != blocker)
                .collect();
            if without.len() != incumbent.len() {
                out.push(without.clone());
            }
            let mut attackers = targets[&blocker].clone();
            attackers.sort_unstable();
            for attacker in attackers {
                let mut with = without.clone();
                with.push((blocker, attacker));
                out.push(with);
            }
        }
    }
    let mut seen: Vec<Vec<(ObjectId, ObjectId)>> = Vec::new();
    out.retain(|declaration| {
        let mut key = declaration.clone();
        key.sort_unstable();
        if seen.contains(&key) {
            false
        } else {
            seen.push(key);
            true
        }
    });
    out
}

/// Measure the heuristic block against its alternatives and return the one to
/// play. `complete` turns raw assignments into the engine action the root would
/// submit (block-tax planning included).
pub(crate) fn refine_block(
    input: &CombatSimInput<'_>,
    incumbent: GameAction,
    complete: impl Fn(&[(ObjectId, ObjectId)]) -> GameAction,
) -> GameAction {
    let Some(opponent) = opponent_of(input.state, input.player) else {
        return incumbent;
    };
    let WaitingFor::DeclareBlockers {
        player,
        valid_block_targets,
        ..
    } = &input.state.waiting_for
    else {
        return incumbent;
    };
    if *player != input.player {
        return incumbent;
    }
    let GameAction::DeclareBlockers { assignments } = &incumbent else {
        return incumbent;
    };

    let mut budget = Budget {
        replays: 0,
        deadline: input.deadline,
    };
    let Some(incumbent_value) = block_value(input, opponent, &incumbent, &mut budget) else {
        return incumbent;
    };

    let mut best: Option<(f64, GameAction)> = None;
    for declaration in
        block_alternatives(input.state, input.player, assignments, valid_block_targets)
            .into_iter()
            .skip(1)
    {
        let action = complete(&declaration);
        if action == incumbent || best.as_ref().is_some_and(|(_, b)| *b == action) {
            continue;
        }
        let Some(value) = block_value(input, opponent, &action, &mut budget) else {
            continue;
        };
        if best.as_ref().is_none_or(|(b, _)| value > *b) {
            best = Some((value, action));
        }
    }
    match best {
        Some((value, action)) if value - incumbent_value >= SWITCH_MARGIN => action,
        _ => incumbent,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn survival_penalty_is_concave_and_capped_at_a_lethal_swing() {
        let w = 0.5;
        assert_eq!(survival_penalty(20, 0, w), 0.0);
        assert_eq!(survival_penalty(5, 5, w), LETHAL_NEXT_SWING);
        assert_eq!(survival_penalty(5, 9, w), LETHAL_NEXT_SWING);
        // The same 4 damage hurts more the closer it leaves you to zero.
        assert!(survival_penalty(6, 4, w) > survival_penalty(20, 4, w));
        assert!(survival_penalty(5, 4, w) > survival_penalty(6, 4, w));
        assert!(survival_penalty(5, 4, w) < LETHAL_NEXT_SWING);
    }

    #[test]
    fn attack_alternatives_lead_with_the_incumbent_and_drop_duplicates() {
        let opp = PlayerId(1);
        let a = ObjectId(10);
        let b = ObjectId(11);
        let to = AttackTarget::Player(opp);
        let alts = attack_alternatives(&[(a, to)], &[a, b], opp);
        assert_eq!(alts[0], vec![(a, to)]);
        assert!(alts.contains(&Vec::new()));
        // "everyone" and "incumbent + b" are the same set: kept once.
        let both = alts.iter().filter(|attacks| attacks.len() == 2).count();
        assert_eq!(both, 1);
        assert!(alts.len() <= MAX_ATTACK_ALTERNATIVES);
    }
}
