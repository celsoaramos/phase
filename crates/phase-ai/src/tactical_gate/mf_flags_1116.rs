//! Sacrifice-for-a-counter outside any window (MagicFinder "jogada ruim do robô"
//! flag f4f895c5, 16/11/2026). In its own draw step, with an empty stack and no
//! combat, the robot activated Arcbound Ravager ("Sacrifice an artifact: Put a +1/+1
//! counter on ~") and paid with the 6/6 Ravager itself, only to move the counters
//! onto a 2/2 Construct; the player also saw it feed artifacts to the Ravager "for no
//! reason" in other games.
//!
//! CR 701.21a + CR 122.1: the ability turns a permanent into one +1/+1 counter. The
//! counters do the same work in combat after blockers are declared, in response to
//! removal, or when the sacrificed artifact has a death payoff; outside of those the
//! activation just throws away a card. This gate rejects the activation from
//! priority only when it can PROVE none of that is in play — empty stack, no combat,
//! and no zone-change/sacrifice trigger on the AI's battlefield other than Modular
//! (which only moves the same counters). Anything else keeps the action (fail open).

use engine::game::functioning_abilities::battlefield_active_triggers;
use engine::types::ability::{AbilityCost, Effect, TargetFilter};
use engine::types::actions::GameAction;
use engine::types::counter::CounterType;
use engine::types::game_state::{GameState, WaitingFor};
use engine::types::keywords::KeywordKind;
use engine::types::player::PlayerId;
use engine::types::triggers::TriggerMode;

use super::GateDecision;
use crate::policies::context::PolicyContext;

/// `ActivateAbility` announced from priority. `Some(Reject)` removes the candidate.
pub(super) fn assess_sacrifice_for_counter(ctx: &PolicyContext<'_>) -> Option<GateDecision> {
    let GameAction::ActivateAbility { source_id, .. } = &ctx.candidate.action else {
        return None;
    };
    if !matches!(ctx.decision.waiting_for, WaitingFor::Priority { .. }) {
        return None;
    }
    let source = ctx.state.objects.get(source_id)?;
    if source.controller != ctx.ai_player {
        return None;
    }
    let ability = ctx.effective_activated_ability()?;
    if !is_sacrifice_for_own_counter(&ability) {
        return None;
    }
    if sacrifice_has_a_window(ctx.state, ctx.ai_player) {
        return None;
    }
    if std::env::var_os("MF_GATE_TRACE").is_some() {
        eprintln!("mf_flags_1116 reject sacrifice-for-counter on {source_id:?}: no window");
    }
    Some(GateDecision::Reject)
}

/// "Sacrifice a <permanent>: Put N +1/+1 counter(s) on ~" with nothing else: the cost
/// is a single sacrifice of another permanent and the whole effect is a +1/+1 counter
/// on the source.
pub fn is_sacrifice_for_own_counter(ability: &engine::types::ability::AbilityDefinition) -> bool {
    let Some(AbilityCost::Sacrifice(cost)) = &ability.cost else {
        return false;
    };
    if matches!(cost.target, TargetFilter::SelfRef) {
        return false;
    }
    if ability.sub_ability.is_some() || ability.else_ability.is_some() || ability.condition.is_some()
    {
        return false;
    }
    matches!(
        ability.effect.as_ref(),
        Effect::PutCounter {
            counter_type: CounterType::Plus1Plus1,
            target: TargetFilter::SelfRef,
            ..
        }
    )
}

/// True when something can make the sacrifice worth an artifact right now. Fails
/// OPEN: any stack, any combat, any death/sacrifice trigger the AI controls (other
/// than Modular's own counter move) counts as a window.
fn sacrifice_has_a_window(state: &GameState, player: PlayerId) -> bool {
    // A spell or ability on the stack: responding (fodder targeted, removal on the
    // source, a pump to race) is the classic use.
    if !state.stack.is_empty() {
        return true;
    }
    // Combat: the counters change blocks and damage (CR 509 / CR 510).
    if state.combat.is_some() {
        return true;
    }
    battlefield_active_triggers(state).any(|(source, active)| {
        if source.controller != player {
            return false;
        }
        let trigger = active.definition;
        let reads_leaving = matches!(
            trigger.mode,
            TriggerMode::ChangesZone
                | TriggerMode::ChangesZoneAll
                | TriggerMode::LeavesBattlefield
                | TriggerMode::Sacrificed
                | TriggerMode::SacrificedOnce
        );
        reads_leaving && !is_modular_trigger(source, trigger)
    })
}

/// CR 702.43a: Modular's dies trigger only moves the source's +1/+1 counters to an
/// artifact creature — the same counters the sacrifice already bought, so it is no
/// payoff for sacrificing.
fn is_modular_trigger(
    source: &engine::game::game_object::GameObject,
    trigger: &engine::types::ability::TriggerDefinition,
) -> bool {
    source.has_keyword_kind(KeywordKind::Modular)
        && matches!(trigger.mode, TriggerMode::ChangesZone)
        && matches!(trigger.valid_card, Some(TargetFilter::SelfRef))
        && trigger.execute.as_ref().is_some_and(|exec| {
            matches!(
                exec.effect.as_ref(),
                Effect::PutCounter {
                    counter_type: CounterType::Plus1Plus1,
                    ..
                }
            )
        })
}
