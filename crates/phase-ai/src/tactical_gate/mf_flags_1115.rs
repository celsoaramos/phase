//! Saddle with no attack left (MagicFinder "jogada ruim do robô" flags 798f877a and
//! 5ea1eee8, 21/10/2026). The robot saddled Bulwark Ox — tapping Canyon Vaulter, and
//! in the second game Interface Ace, Voyager Glidecar and the Vaulter — and the Ox
//! never attacked: the saved states show `is_saddled` with empty
//! `creatures_attacked_this_turn` in the postcombat main phase. The saddlers stay
//! tapped through the opponent's turn, so the robot lost its blockers at 8 life
//! against 8 power for nothing.
//!
//! CR 702.171a: saddling only makes the Mount "saddled until end of turn"; what a
//! saddled Mount gains is "attacks while saddled" triggers and "as long as ~ is
//! saddled" statics. With no attack left for the Mount this turn, the designation
//! is worthless unless something else reads it — a saddle trigger with a lasting
//! effect, a static conditioned on it, or a tap trigger on a creature that pays.
//! This gate rejects `SaddleMount` from priority only when it can PROVE all of that
//! is absent; anything it cannot read keeps the action (fail open).

use engine::game::combat::{get_valid_attacker_ids, has_summoning_sickness};
use engine::game::functioning_abilities::battlefield_active_triggers;
use engine::types::ability::{
    AbilityDefinition, ContinuousModification, Duration, Effect, StaticCondition, TapStateChange,
    TargetFilter, TriggerDefinition,
};
use engine::types::actions::GameAction;
use engine::types::card_type::CoreType;
use engine::types::game_state::{GameState, WaitingFor};
use engine::types::identifiers::ObjectId;
use engine::types::keywords::Keyword;
use engine::types::phase::Phase;
use engine::types::player::PlayerId;
use engine::types::triggers::TriggerMode;

use super::GateDecision;
use crate::policies::context::PolicyContext;

/// `SaddleMount` announced from priority. `Some(Reject)` removes the candidate.
pub(super) fn assess_saddle(ctx: &PolicyContext<'_>) -> Option<GateDecision> {
    let GameAction::SaddleMount { mount_id, .. } = &ctx.candidate.action else {
        return None;
    };
    // Only the activation itself; choosing the creatures (`WaitingFor::SaddleMount`)
    // happens after the decision to saddle was already taken.
    if !matches!(ctx.decision.waiting_for, WaitingFor::Priority { .. }) {
        return None;
    }
    if saddle_has_no_attack_left(ctx.state, ctx.ai_player, *mount_id) {
        if std::env::var_os("MF_GATE_TRACE").is_some() {
            eprintln!("mf_flags_1115 reject SaddleMount {mount_id:?}: no attack left");
        }
        return Some(GateDecision::Reject);
    }
    None
}

/// True when saddling `mount_id` now is provably worthless: the Mount cannot attack
/// for the rest of this turn and nothing else reads the saddle.
pub fn saddle_has_no_attack_left(state: &GameState, player: PlayerId, mount_id: ObjectId) -> bool {
    // Saddle is sorcery-speed (CR 702.171a): the active player in a main phase.
    if state.active_player != player {
        return false;
    }
    let Some(mount) = state.objects.get(&mount_id) else {
        return false;
    };
    if mount.controller != player {
        return false;
    }
    if mount_can_still_attack(state, mount_id) {
        return false;
    }
    !saddle_has_value_without_attack(state, player, mount_id)
}

/// CR 506.1 + CR 500.8: the only combat left after the postcombat main phase is an
/// extra one; with none scheduled the turn's attacks are over. Before combat, the
/// Mount still attacks only if it is an eligible attacker (untapped, no summoning
/// sickness, no defender — CR 508.1a / CR 302.6).
fn mount_can_still_attack(state: &GameState, mount_id: ObjectId) -> bool {
    match state.phase {
        Phase::PreCombatMain => {}
        Phase::PostCombatMain if !state.extra_phases.is_empty() => return true,
        Phase::PostCombatMain => return false,
        // Any other window: not a saddle window we model — fail open.
        _ => return true,
    }
    let Some(mount) = state.objects.get(&mount_id) else {
        return true;
    };
    if !mount.card_types.core_types.contains(&CoreType::Creature) {
        // A Mount that is not a creature right now (animated later?) — fail open.
        return true;
    }
    if mount.tapped || has_summoning_sickness(mount) {
        return false;
    }
    get_valid_attacker_ids(state).contains(&mount_id)
}

/// Anything other than "attacks while saddled" that a saddle could feed. Fails OPEN:
/// every shape it does not recognise as worthless counts as value.
fn saddle_has_value_without_attack(state: &GameState, player: PlayerId, mount_id: ObjectId) -> bool {
    let Some(mount) = state.objects.get(&mount_id) else {
        return true;
    };
    // "As long as ~ is saddled, …" (CR 702.171b statics).
    if mount
        .static_definitions
        .iter_unchecked()
        .any(|stat| stat.condition.as_ref().is_some_and(condition_reads_saddled))
    {
        return true;
    }
    battlefield_active_triggers(state).any(|(source, active)| {
        let trigger = active.definition;
        match trigger.mode {
            TriggerMode::Saddled
            | TriggerMode::Saddles
            | TriggerMode::SaddlesOrCrews
            | TriggerMode::BecomesSaddled => !execute_is_combat_only_grant(trigger),
            // The saddle cost taps creatures: a tap trigger on one of the
            // player's creatures that could pay is a reason to tap it — unless
            // it only untaps itself (Interface Ace), which is no gain.
            TriggerMode::Taps => {
                source.controller == player
                    && source.id != mount_id
                    && source.card_types.core_types.contains(&CoreType::Creature)
                    && !execute_only_untaps_self(trigger)
            }
            _ => false,
        }
    })
}

fn condition_reads_saddled(condition: &StaticCondition) -> bool {
    match condition {
        StaticCondition::SourceIsSaddled => true,
        StaticCondition::And { conditions } | StaticCondition::Or { conditions } => {
            conditions.iter().any(condition_reads_saddled)
        }
        StaticCondition::Not { condition } => condition_reads_saddled(condition),
        _ => false,
    }
}

/// Canyon Vaulter: "that Mount or Vehicle gains flying until end of turn". A
/// combat-only keyword until end of turn, granted in the active player's own turn
/// with no attack left, does nothing (it is gone before the opponent attacks).
fn execute_is_combat_only_grant(trigger: &TriggerDefinition) -> bool {
    let Some(execute) = trigger.execute.as_deref() else {
        return false;
    };
    ability_is_combat_only_grant(execute)
}

fn ability_is_combat_only_grant(ability: &AbilityDefinition) -> bool {
    if ability.sub_ability.is_some() || ability.else_ability.is_some() {
        return false;
    }
    let Effect::GenericEffect {
        static_abilities,
        duration,
        ..
    } = ability.effect.as_ref()
    else {
        return false;
    };
    let until_end_of_turn = matches!(duration, Some(Duration::UntilEndOfTurn))
        || matches!(ability.duration, Some(Duration::UntilEndOfTurn));
    until_end_of_turn
        && !static_abilities.is_empty()
        && static_abilities.iter().all(|stat| {
            !stat.modifications.is_empty()
                && stat.modifications.iter().all(|modification| {
                    matches!(
                        modification,
                        ContinuousModification::AddKeyword { keyword } if is_combat_only_keyword(keyword)
                    )
                })
        })
}

/// Keywords that only matter while attacking or blocking. Haste (unlocks {T}
/// abilities), lifelink and deathtouch (fights, pings) are deliberately absent.
fn is_combat_only_keyword(keyword: &Keyword) -> bool {
    matches!(
        keyword,
        Keyword::Flying
            | Keyword::Trample
            | Keyword::Menace
            | Keyword::FirstStrike
            | Keyword::DoubleStrike
            | Keyword::Vigilance
            | Keyword::Reach
    )
}

/// Interface Ace: "Whenever this creature becomes tapped during your turn, untap it."
fn execute_only_untaps_self(trigger: &TriggerDefinition) -> bool {
    let Some(execute) = trigger.execute.as_deref() else {
        return false;
    };
    execute.sub_ability.is_none()
        && execute.else_ability.is_none()
        && matches!(
            execute.effect.as_ref(),
            Effect::SetTapState {
                target: TargetFilter::SelfRef,
                state: TapStateChange::Untap,
                ..
            }
        )
}
