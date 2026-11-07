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
//! CR 502.3 untaps them only in their controller's untap step), plus the same
//! term for the next swing that board makes at the opponent.
//!
//! A prompt that belongs to the simulating player mid-replay ("you may put an
//! Angel, Demon, or Dragon onto the battlefield attacking" — Kaalia of the
//! Vast; a trigger's target) is answered by the AI's own quick choice
//! (`CombatSimInput::answer`). Before that answer existed, every such prompt
//! dropped the declaration, and Kaalia never attacked at the "Difícil" table
//! (six flags in one game, 2026-10-29). The OTHER player's prompts are answered
//! only when the engine issues a single legal action: picking for them would
//! read their hidden hand. Anything else drops that candidate. When the
//! incumbent itself cannot be replayed, the heuristic answer stands unchanged:
//! this layer only ever swaps a declaration it has measured for a better one.
//!
//! The replay never sees the other player's hand, so on its own it assumes
//! they hold nothing. With open mana that is the trade a person wins with a
//! Giant Growth. When the difficulty-gated threat profile says a combat trick
//! is castable (`CombatSimInput::trick`), every blocked exchange is also played
//! with the opponent's best pump on one of its creatures in the combat (an
//! instant after blockers are declared, CR 509.4 + CR 117.3b), and the
//! exchange is worth `(1 − p) · plain + p · min(plain, pumped)`. Only the
//! probability comes from the profile; which creature gets the pump is the
//! opponent's best pick, as with blocks.

use std::collections::HashMap;

use engine::ai_support::legal_actions;
use engine::game::combat::{
    complete_blocker_proposal, pending_combat_tax_is_affordable, validate_blockers_for_player,
    AttackTarget, CombatTaxPosture,
};
use engine::game::engine::apply_as_current_for_simulation;
use engine::game::layers::flush_layers;
use engine::game::players;
use engine::types::actions::GameAction;
use engine::types::counter::CounterType;
use engine::types::game_state::{CombatDamageAssignmentMode, GameState, WaitingFor};
use engine::types::identifiers::ObjectId;
use engine::types::phase::Phase;
use engine::types::player::PlayerId;
use engine::util::Deadline;

use crate::combat_ai::{choose_blockers_with_profile, crackback_damage, latent_blockers};
use crate::config::AiProfile;
use crate::eval::{evaluate_creature, evaluate_state, EvalWeights};
use engine::types::ability::{DelayedTriggerCondition, Effect, TargetFilter};
use engine::types::keywords::Keyword;
use engine::types::triggers::TriggerMode;
use engine::types::zones::Zone;

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
const MAX_REPLAYS: usize = 240;
/// Opponent creatures in a blocked exchange that are tried as the pump target.
const MAX_TRICK_TARGETS: usize = 2;
/// An alternative must beat the incumbent by this much (evaluation units: a
/// point of life is ~0.5, a 2/2 ~6) — measurement noise must not churn the
/// heuristic's choice.
pub(crate) const SWITCH_MARGIN: f64 = 1.0;
/// Penalty for a board the opponent can kill next swing (≈ four creatures:
/// well below a real loss, well above any trade).
const LETHAL_NEXT_SWING: f64 = 30.0;
/// Below this life total every point costs more than the board shows (CR 704.5a
/// is one swing away). Flag 343d16d7 (04/11/2026, MagicFinder): at 14 life
/// facing an 11/10 lifelink the replay kept its 1/1 home and went to 3 — the
/// next-swing term saw the 1/1 still able to chump NEXT turn, so the 11 life
/// cost only its evaluation weight. The floor prices the life itself.
const LOW_LIFE_FLOOR: i32 = 8;
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
    /// The AI's own answer (a small search, no nested combat replay) to a
    /// prompt of `player`'s that opens mid-replay. `None` drops the candidate.
    pub answer: &'a dyn Fn(&GameState) -> Option<GameAction>,
    /// The opponent may hold a combat trick. `None` = it can't (no open mana,
    /// no hand, or a profile that rules it out).
    pub trick: Option<TrickRisk>,
}

/// The combat trick the replay accounts for: a pump of `size` (+N/+N until
/// end of turn) the opponent holds with `probability`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct TrickRisk {
    pub probability: f64,
    pub size: u32,
}

/// Ceiling on the trick probability. The pump is played as the opponent's
/// best pick, so the probability only weighs it — a strong prior here would
/// stop every attack into open mana.
const MAX_TRICK_PROBABILITY: f64 = 0.35;

impl TrickRisk {
    /// `raw` = P(a castable combat trick in hand), from the threat profile.
    /// The pump is +2/+2 with one open mana and +3/+3 with two or more —
    /// the sizes of the cheap tricks people actually play.
    pub(crate) fn from_probability(raw: f64, scale: f64, open_mana: u32) -> Option<Self> {
        if open_mana == 0 {
            return None;
        }
        let probability = (raw * scale).clamp(0.0, MAX_TRICK_PROBABILITY);
        (probability > 0.0).then_some(TrickRisk {
            probability,
            size: if open_mana >= 2 { 3 } else { 2 },
        })
    }
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
fn advance(
    input: &CombatSimInput<'_>,
    state: &mut GameState,
    turn: u32,
    stop_at_blocks: bool,
) -> Reached {
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
            // CR 603.3b: simultaneous triggers go on the stack in an order their
            // controller picks. Combat damage triggers (No Mercy, "whenever a
            // creature dies") are what a combat is often decided by, so take the
            // printed order instead of dropping the declaration.
            WaitingFor::OrderTriggers { triggers, .. } => GameAction::OrderTriggers {
                order: (0..triggers.len()).collect(),
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
            _ => match answer_prompt(input, state) {
                Some(action) => action,
                None => {
                    trace(|| format!("combat_sim unsupported prompt {:?}", state.waiting_for.variant_name()));
                    return Reached::Unsupported;
                }
            },
        };
        let described = format!("{action:?}");
        if apply_as_current_for_simulation(state, action).is_err() {
            trace(|| format!("combat_sim replay refused {described} at {:?}", state.waiting_for.variant_name()));
            return Reached::Unsupported;
        }
    }
    Reached::Unsupported
}

/// A mid-replay prompt the loop above has no rule for. The simulating player's
/// own prompt gets the AI's quick choice; the other player's only a forced one.
/// Opponent prompts with more than one answer the replay still measures: each
/// answer is applied and the one that leaves the AI WORST off is taken (the
/// CR 509.1 worst-case stance the attack branch already uses). Above this many
/// answers the line is dropped as before — a Krenko board's trigger targets
/// would otherwise multiply the replays. "Any target" on a two-player board
/// with a handful of creatures is a dozen answers.
const MAX_OPPONENT_PROMPT_ANSWERS: usize = 16;

fn answer_prompt(input: &CombatSimInput<'_>, state: &GameState) -> Option<GameAction> {
    if state.waiting_for.acting_player() == Some(input.player) {
        return (input.answer)(state);
    }
    let actions = legal_actions(state);
    match actions.len() {
        0 => None,
        1 => actions.into_iter().next(),
        n if n <= MAX_OPPONENT_PROMPT_ANSWERS => {
            // Flag 58ef961d (31/10/2026): two doomed 3/1s gang-blocking a 5/5
            // whose death trigger ("deals 3 damage to any target") asks the
            // opponent for a target — the only block that killed it was the
            // one the replay could not measure.
            let mut worst: Option<(f64, GameAction)> = None;
            for action in actions {
                let mut branch = state.clone();
                if apply_as_current_for_simulation(&mut branch, action.clone()).is_err() {
                    continue;
                }
                let value = evaluate_state(&branch, input.player, input.weights);
                if worst.as_ref().is_none_or(|(w, _)| value < *w) {
                    worst = Some((value, action));
                }
            }
            worst.map(|(_, action)| action)
        }
        _ => None,
    }
}

fn set_pump(state: &mut GameState, id: ObjectId, size: u32, on: bool) -> bool {
    let Some(object) = state.objects.get_mut(&id) else {
        return false;
    };
    let counters = object.counters.entry(CounterType::Plus1Plus1).or_insert(0);
    if on {
        *counters += size;
    } else {
        *counters = counters.saturating_sub(size);
        if *counters == 0 {
            object.counters.remove(&CounterType::Plus1Plus1);
        }
    }
    state.layers_dirty.mark_full();
    flush_layers(state);
    true
}

/// The opponent's creatures in this combat a pump would matter on, most
/// valuable exchange first: on attack, its blockers; on block, its attackers.
fn trick_targets(state: &GameState, opponent: PlayerId, attacking: bool) -> Vec<ObjectId> {
    let Some(combat) = state.combat.as_ref() else {
        return Vec::new();
    };
    let size = |id: ObjectId| {
        state
            .objects
            .get(&id)
            .map_or(0, |o| o.power.unwrap_or(0) + o.toughness.unwrap_or(0))
    };
    let mut out: Vec<(i32, ObjectId)> = if attacking {
        combat
            .blocker_to_attacker
            .iter()
            .filter(|(blocker, _)| {
                state
                    .objects
                    .get(blocker)
                    .is_some_and(|o| o.controller == opponent)
            })
            .map(|(&blocker, attackers)| {
                (
                    attackers.iter().map(|&a| size(a)).max().unwrap_or(0),
                    blocker,
                )
            })
            .collect()
    } else {
        combat
            .attackers
            .iter()
            .filter(|a| {
                state
                    .objects
                    .get(&a.object_id)
                    .is_some_and(|o| o.controller == opponent)
            })
            .map(|a| (size(a.object_id), a.object_id))
            .collect()
    };
    out.sort_unstable_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    out.into_iter()
        .map(|(_, id)| id)
        .take(MAX_TRICK_TARGETS)
        .collect()
}

/// Play the rest of combat from `branch` (blocks declared) and score it, with
/// the trick risk folded in: `(1 − p) · plain + p · min(plain, pumped)`.
/// `None` when a replay can't be finished honestly.
fn finish_with_trick(
    input: &CombatSimInput<'_>,
    opponent: PlayerId,
    branch: &GameState,
    turn: u32,
    attacking: bool,
    budget: &mut Budget,
) -> Option<f64> {
    let untap_mine = !attacking;
    let mut plain = branch.clone();
    match advance(input, &mut plain, turn, false) {
        Reached::Finished => {}
        _ => return None,
    }
    let plain_value = position_value(&plain, input.player, opponent, input.weights, untap_mine);
    trace(|| {
        let board: Vec<String> = plain
            .battlefield
            .iter()
            .filter_map(|id| plain.objects.get(id))
            .filter(|o| o.card_types.core_types.contains(&engine::types::card_type::CoreType::Creature))
            .map(|o| format!("{}@{} {}/{} dmg{}{}", o.name, o.controller.0, o.power.unwrap_or(0), o.toughness.unwrap_or(0), o.damage_marked, if o.tapped { " T" } else { "" }))
            .collect();
        format!(
            "combat_sim     after combat life {}×{} value {plain_value:.3} board {board:?}",
            plain.players[input.player.0 as usize].life,
            plain.players[opponent.0 as usize].life
        )
    });
    let Some(trick) = input.trick else {
        return Some(plain_value);
    };
    if plain_value.abs() >= DECIDED {
        return Some(plain_value);
    }
    let mut pumped_worst = plain_value;
    for target in trick_targets(branch, opponent, attacking) {
        if !budget.take() {
            // A partial worst case would flatter this declaration.
            return None;
        }
        let mut pumped = branch.clone();
        if !set_pump(&mut pumped, target, trick.size, true) {
            continue;
        }
        if !matches!(advance(input, &mut pumped, turn, false), Reached::Finished) {
            continue;
        }
        // The pump ends with the turn (CR 514.2): score the board without it.
        if pumped.battlefield.contains(&target) {
            set_pump(&mut pumped, target, trick.size, false);
        }
        let value = position_value(&pumped, input.player, opponent, input.weights, untap_mine);
        pumped_worst = pumped_worst.min(value);
    }
    Some(plain_value - trick.probability * (plain_value - pumped_worst))
}

/// Damage `attacker`'s whole creature board can push through `defender`'s
/// untapped creatures on `attacker`'s next swing. `untap_defender` = the
/// defender untaps before that swing (its turn comes first).
fn next_swing(
    state: &GameState,
    defender: PlayerId,
    attacker: PlayerId,
    untap_defender: bool,
) -> i32 {
    let mut view = state.clone();
    let battlefield: Vec<ObjectId> = view.battlefield.iter().copied().collect();
    for id in battlefield {
        if let Some(object) = view.objects.get_mut(&id) {
            if object.controller == attacker || (untap_defender && object.controller == defender) {
                object.tapped = false;
            }
        }
    }
    crackback_damage(&view, defender, &[attacker], &[], None)
}

/// Quadratic cost of ending the combat at `life` below [`LOW_LIFE_FLOOR`]:
/// 0 at the floor, 12.5 at 3, 24.5 at 1. Zero at or below 0 (the game is decided
/// and `evaluate_state` already says so).
pub(crate) fn low_life_penalty(life: i32) -> f64 {
    if life <= 0 || life >= LOW_LIFE_FLOOR {
        return 0.0;
    }
    let short = f64::from(LOW_LIFE_FLOOR - life);
    0.5 * short * short
}

/// CR 603.7 + CR 514.2: a creature its own end-step trigger, or a delayed
/// trigger, sacrifices or exiles "at the beginning of the (next) end step" is
/// gone before it can ever block or attack again (Ball Lightning, Elemental
/// tokens from "sacrifice it at the beginning of the next end step"). Flag
/// 58ef961d (31/10/2026): two such 3/1s stayed home while a 5/5 walked in — the
/// replay counted them as survivors worth keeping. Fail open: a doomed creature
/// the engine represents some other way is simply priced as a normal one.
pub(crate) fn doomed_at_end_step(state: &GameState, id: ObjectId) -> bool {
    let Some(object) = state.objects.get(&id) else {
        return false;
    };
    let dooms_self = |effect: &Effect| -> bool {
        matches!(
            effect,
            Effect::Sacrifice {
                target: TargetFilter::SelfRef,
                ..
            } | Effect::ChangeZone {
                destination: Zone::Exile | Zone::Graveyard,
                target: TargetFilter::SelfRef,
                ..
            }
        )
    };
    let own_trigger = object.trigger_definitions.iter_unchecked().any(|entry| {
        let def = &entry.definition;
        matches!(def.mode, TriggerMode::Phase)
            && matches!(def.phase, Some(Phase::End))
            && def
                .execute
                .as_deref()
                .is_some_and(|execute| crate::cast_facts::collect_definition_effects(execute).into_iter().any(dooms_self))
    });
    if own_trigger {
        return true;
    }
    let names_me = |filter: &TargetFilter| -> bool {
        match filter {
            TargetFilter::SpecificObject { id: named } => *named == id,
            TargetFilter::LastCreated => state.last_created_token_ids.contains(&id),
            _ => false,
        }
    };
    state.delayed_triggers.iter().any(|delayed| {
        let at_end = matches!(
            delayed.condition,
            DelayedTriggerCondition::AtNextPhase { phase: Phase::End }
                | DelayedTriggerCondition::AtNextPhaseForPlayer { phase: Phase::End, .. }
        );
        at_end
            && crate::policies::context::collect_ability_effects(&delayed.ability)
                .into_iter()
                .any(|effect| match effect {
                    Effect::Sacrifice { target, .. } => names_me(target),
                    Effect::ChangeZone {
                        destination: Zone::Exile | Zone::Graveyard,
                        target,
                        ..
                    } => names_me(target),
                    _ => false,
                })
    })
}

/// Evaluation the board credits `player` for creatures that will not survive the
/// turn: subtracted from the position so a doomed body spent in combat costs
/// nothing and one kept home buys nothing.
fn doomed_value(state: &GameState, player: PlayerId) -> f64 {
    state
        .battlefield
        .iter()
        .filter(|id| {
            state
                .objects
                .get(id)
                .is_some_and(|object| object.controller == player && doomed_at_end_step(state, **id))
        })
        .map(|id| evaluate_creature(state, *id))
        .sum()
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
    // CR 502.3: every permanent untaps in its controller's next untap step, so
    // the tapped state a combat leaves is transient — but `evaluate_creature`
    // charges `tapped_penalty` for it, and only the ATTACKER's creatures are
    // tapped after combat (blockers don't tap). Read the board untapped on both
    // sides; the next-swing terms below already carry who untaps first. Flag
    // 21537628 (28/10/2026): a 3/4 trample stayed home against a lone 3/3
    // because attacking cost it the tapped discount and nothing else.
    let mut untapped = state.clone();
    let battlefield: Vec<ObjectId> = untapped.battlefield.iter().copied().collect();
    for id in battlefield {
        if let Some(object) = untapped.objects.get_mut(&id) {
            object.tapped = false;
        }
    }
    let base = evaluate_state(&untapped, player, weights);
    if base.abs() >= DECIDED {
        return base;
    }
    // Both next swings, read off the board this combat leaves. CR 502.3: each
    // player untaps only in its own untap step, so whoever's turn comes first
    // faces the other's creatures still tapped from this combat. Attacking now
    // (`untap_mine` false), the opponent untaps before its swing and before
    // ours; blocking now, we untap first and its attackers can't block us.
    //
    // The swing at us is the risk the combat leaves; the swing at the opponent
    // is what it buys, priced by the same concave curve. Without that credit,
    // a declaration that forces the defender to chump its board away (CR
    // 509.1a — 5 life behind four blockers facing four attackers) scored as
    // the creature it costs, and "stay home" won over an attack that leaves
    // the opponent dead to the next swing.
    let life = state.players[player.0 as usize].life;
    let opponent_life = state.players[opponent.0 as usize].life;
    base - survival_penalty(
        life,
        next_swing(state, player, opponent, untap_mine),
        weights.life,
    ) + survival_penalty(
        opponent_life,
        next_swing(state, opponent, player, !untap_mine),
        weights.life,
    ) - low_life_penalty(life)
        + low_life_penalty(opponent_life)
        - doomed_value(state, player)
        + doomed_value(state, opponent)
}

/// Blockers that together kill `attacker`, doomed bodies first, then the biggest
/// hitters — the smallest such group of two or more. `None` when no group kills
/// it or one blocker already does (the one-change alternatives cover that).
fn gang_block(
    state: &GameState,
    attacker: ObjectId,
    blockers: &[ObjectId],
) -> Option<Vec<ObjectId>> {
    let target = state.objects.get(&attacker)?;
    if target.has_keyword(&Keyword::Indestructible) {
        return None;
    }
    let toughness = target.toughness? - i32::try_from(target.damage_marked).unwrap_or(0);
    if toughness <= 0 {
        return None;
    }
    let power = |id: &ObjectId| state.objects.get(id).and_then(|o| o.power).unwrap_or(0);
    let mut ordered: Vec<ObjectId> = blockers.to_vec();
    ordered.sort_by(|a, b| {
        doomed_at_end_step(state, *b)
            .cmp(&doomed_at_end_step(state, *a))
            .then_with(|| power(b).cmp(&power(a)))
    });
    let mut group = Vec::new();
    let mut total = 0;
    for id in ordered {
        if power(&id) <= 0 {
            continue;
        }
        group.push(id);
        total += power(&id);
        if total >= toughness {
            break;
        }
    }
    (total >= toughness && group.len() >= 2 && group.len() <= 4).then_some(group)
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
    let (defender, targets) = match advance(input, &mut declared, turn, true) {
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
        // The defender's answer led to a prompt the replay will not answer
        // (a trigger target, a damage-assignment order): this attack can't be
        // measured honestly.
        let measured = finish_with_trick(input, opponent, &branch, turn, true, budget);
        trace(|| format!("combat_sim   defender answer {answer:?} => {measured:?}"));
        let value = measured?;
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
    // CR 602.2: a land the defender can animate with open mana is a blocker the
    // replay never sees — it only passes priority. The heuristic already counts
    // those bodies (`latent_blockers`), so its answer stands.
    if !latent_blockers(input.state, opponent).is_empty() {
        return incumbent;
    }

    let mut budget = Budget {
        replays: 0,
        deadline: input.deadline,
    };
    let incumbent_measured = attack_value(input, opponent, &incumbent, &mut budget);
    trace(|| format!("combat_sim attack incumbent {incumbent:?} => {incumbent_measured:?}"));
    let Some(incumbent_value) = incumbent_measured else {
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
        let measured = attack_value(input, opponent, &action, &mut budget);
        trace(|| format!("combat_sim attack alternative {action:?} => {measured:?}"));
        let Some(value) = measured else {
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
    finish_with_trick(input, opponent, &branch, turn, false, budget)
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
    // Gang blocks (CR 509.1a lets several creatures block one attacker): the
    // one-blocker changes above never reach "these two together kill it", which
    // is exactly the block two doomed 3/1s owed a 5/5 (flag 58ef961d).
    let mut attackers: Vec<ObjectId> = targets.values().flatten().copied().collect();
    attackers.sort_unstable();
    attackers.dedup();
    for attacker in attackers {
        let able: Vec<ObjectId> = targets
            .iter()
            .filter(|(_, legal)| legal.contains(&attacker))
            .map(|(blocker, _)| *blocker)
            .collect();
        let Some(group) = gang_block(state, attacker, &able) else {
            continue;
        };
        let mut declaration: Vec<(ObjectId, ObjectId)> = incumbent
            .iter()
            .copied()
            .filter(|(b, _)| !group.contains(b))
            .collect();
        declaration.extend(group.iter().map(|b| (*b, attacker)));
        out.push(declaration);
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

/// `COMBAT_SIM_TRACE=1` prints every alternative's value (native only; the
/// browser has no environment and stays silent).
fn trace(message: impl FnOnce() -> String) {
    if std::env::var_os("COMBAT_SIM_TRACE").is_some() {
        eprintln!("{}", message());
    }
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
    let incumbent_measured = block_value(input, opponent, &incumbent, &mut budget);
    trace(|| format!("combat_sim block incumbent {incumbent:?} => {incumbent_measured:?}"));
    let Some(incumbent_value) = incumbent_measured else {
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
        let measured = block_value(input, opponent, &action, &mut budget);
        trace(|| format!("combat_sim block alternative {action:?} => {measured:?}"));
        let Some(value) = measured else {
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
    fn low_life_penalty_prices_the_last_points_and_nothing_above_the_floor() {
        assert_eq!(low_life_penalty(LOW_LIFE_FLOOR), 0.0);
        assert_eq!(low_life_penalty(20), 0.0);
        assert_eq!(low_life_penalty(0), 0.0);
        assert!(low_life_penalty(7) > 0.0);
        assert!(low_life_penalty(3) > low_life_penalty(7));
        // Flag 343d16d7: 14 → 3 must cost more than a 1/1 chump blocker (~3).
        assert!(low_life_penalty(3) > 6.0);
    }

    #[test]
    fn end_step_sacrifice_trigger_marks_the_creature_doomed() {
        use engine::game::scenario::{GameScenario, P0, P1};
        let mut s = GameScenario::new();
        s.at_phase(Phase::PreCombatMain);
        let bolt = s
            .add_creature(P0, "Ball Lightning", 6, 1)
            .from_oracle_text("Trample, haste\nAt the beginning of the end step, sacrifice this creature.")
            .id();
        let bear = s.add_creature(P0, "Bear", 2, 2).id();
        let theirs = s.add_creature(P1, "Ogre", 5, 5).id();
        let r = s.build();
        assert!(doomed_at_end_step(r.state(), bolt));
        assert!(!doomed_at_end_step(r.state(), bear));
        assert!(!doomed_at_end_step(r.state(), theirs));
        // The doomed body is worth nothing to the position it leaves.
        assert!(doomed_value(r.state(), P0) > 0.0);
        assert_eq!(doomed_value(r.state(), P1), 0.0);
    }

    #[test]
    fn gang_block_groups_the_doomed_first_and_only_when_the_group_kills() {
        use engine::game::scenario::{GameScenario, P0, P1};
        let mut s = GameScenario::new();
        s.at_phase(Phase::PreCombatMain);
        let a = s
            .add_creature(P0, "Elemental A", 3, 1)
            .from_oracle_text("At the beginning of the end step, sacrifice this creature.")
            .id();
        let b = s
            .add_creature(P0, "Elemental B", 3, 1)
            .from_oracle_text("At the beginning of the end step, sacrifice this creature.")
            .id();
        let mage = s.add_creature(P0, "Guildmage", 2, 2).id();
        let omnath = s.add_creature(P1, "Omnath", 5, 5).id();
        let yargle = s.add_creature(P1, "Yargle", 18, 6).id();
        let r = s.build();
        // 3 + 3 ≥ 5: the two doomed bodies, not the Guildmage.
        assert_eq!(gang_block(r.state(), omnath, &[mage, a, b]), Some(vec![a, b]));
        // 3 + 3 = 6 ≥ 6: the two doomed bodies kill the 18/6 too; the Guildmage stays out.
        assert_eq!(gang_block(r.state(), yargle, &[mage, a, b]), Some(vec![a, b]));
        // A 7-toughness body needs all three (3 + 3 + 2 = 8).
        let big = s_big();
        assert_eq!(gang_block(big.state(), big_target(&big), &big_blockers(&big)).map(|g| g.len()), Some(3));
        // One blocker alone is the one-change alternative's job.
        assert_eq!(gang_block(r.state(), omnath, &[yargle]), None);
        // Nothing kills it: no group.
        assert_eq!(gang_block(r.state(), yargle, &[mage]), None);
    }

    fn s_big() -> engine::game::scenario::GameRunner {
        use engine::game::scenario::{GameScenario, P0, P1};
        let mut s = GameScenario::new();
        s.at_phase(Phase::PreCombatMain);
        s.add_creature(P0, "Elemental A", 3, 1)
            .from_oracle_text("At the beginning of the end step, sacrifice this creature.");
        s.add_creature(P0, "Elemental B", 3, 1)
            .from_oracle_text("At the beginning of the end step, sacrifice this creature.");
        s.add_creature(P0, "Guildmage", 2, 2);
        s.add_creature(P1, "Wurm", 7, 7);
        s.build()
    }
    fn big_target(r: &engine::game::scenario::GameRunner) -> ObjectId {
        *r.state().battlefield.iter().find(|id| r.state().objects[*id].name == "Wurm").unwrap()
    }
    fn big_blockers(r: &engine::game::scenario::GameRunner) -> Vec<ObjectId> {
        r.state()
            .battlefield
            .iter()
            .copied()
            .filter(|id| r.state().objects[id].controller == PlayerId(0))
            .collect()
    }

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

    fn attack_into(
        trick: Option<TrickRisk>,
        mine: (i32, i32),
        theirs: (i32, i32),
        their_life: i32,
    ) -> Vec<ObjectId> {
        use engine::game::scenario::{GameScenario, P0, P1};
        let mut s = GameScenario::new();
        s.at_phase(Phase::PreCombatMain);
        s.with_life(P1, their_life);
        let ogre = s.add_creature(P0, "Ogre", mine.0, mine.1).id();
        s.add_creature(P1, "Bear", theirs.0, theirs.1);
        let mut r = s.build();
        r.advance_to_combat();
        let state = r.state();
        assert!(matches!(
            state.waiting_for,
            WaitingFor::DeclareAttackers { .. }
        ));
        let weights = EvalWeights::default();
        let answer = |_: &GameState| None;
        let input = CombatSimInput {
            state,
            player: P0,
            weights: &weights,
            deadline: None,
            answer: &answer,
            trick,
        };
        let declare = |attacks: Vec<(ObjectId, AttackTarget)>| GameAction::DeclareAttackers {
            attacks,
            bands: vec![],
        };
        let incumbent = declare(vec![(ogre, AttackTarget::Player(P1))]);
        match refine_attack(&input, incumbent, declare) {
            GameAction::DeclareAttackers { attacks, .. } => {
                attacks.into_iter().map(|(id, _)| id).collect()
            }
            other => panic!("expected DeclareAttackers, got {other:?}"),
        }
    }

    /// An even trade is worth offering when the defender holds nothing; into
    /// open mana that may be a Giant Growth it is a 2/2 lost for nothing. A
    /// clearly better attack still goes: a likely pump only tips the close ones.
    #[test]
    fn a_likely_pump_on_the_blocker_keeps_the_even_trade_home() {
        let trick = Some(TrickRisk {
            probability: MAX_TRICK_PROBABILITY,
            size: 3,
        });
        assert_eq!(
            attack_into(None, (2, 2), (2, 2), 20).len(),
            1,
            "no trick: offer the trade"
        );
        assert!(
            attack_into(trick, (2, 2), (2, 2), 20).is_empty(),
            "a +3/+3 on their 2/2 eats ours: stay home"
        );
        for (mine, theirs) in [((3, 3), (2, 2)), ((4, 4), (3, 3)), ((3, 3), (1, 1))] {
            assert_eq!(
                attack_into(trick, mine, theirs, 20).len(),
                1,
                "{mine:?} into {theirs:?} still attacks"
            );
        }
    }

    #[test]
    fn trick_risk_needs_open_mana_and_is_capped() {
        assert_eq!(TrickRisk::from_probability(0.9, 1.0, 0), None);
        let one = TrickRisk::from_probability(0.9, 1.0, 1).unwrap();
        assert_eq!(one.probability, MAX_TRICK_PROBABILITY);
        assert_eq!(one.size, 2);
        assert_eq!(TrickRisk::from_probability(0.2, 1.0, 3).unwrap().size, 3);
        assert_eq!(TrickRisk::from_probability(0.0, 1.0, 3), None);
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
