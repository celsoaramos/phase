//! Sacrifice-for-mana the payment does not need (MagicFinder "jogada ruim do
//! robô" e8fac237, 25/11/2026). Paying Tyvar's Stand ({X}{G}, X = 2) with
//! Ashnod's Altar ("Sacrifice a creature: Add {C}{C}."), the robot kept
//! sacrificing creatures after its mana pool and Arcane Signet already covered
//! the cost: four colorless floating, the attacking Meteor Golem next in line.
//!
//! CR 601.2g-h: the caster activates mana abilities and then pays the total cost.
//! Mana it produces beyond that empties from the pool unused (CR 500.4), and a
//! sacrificed permanent does not come back (CR 701.21a). This gate rejects a
//! mana ability whose cost sacrifices another permanent, during the robot's own
//! `ManaPayment`, only when it can PROVE the sacrifice buys nothing: the engine's
//! automatic payer (which never pays an interactive sacrifice cost) covers the
//! pending cost from the pool plus the robot's other sources, and no permanent
//! the cost could sacrifice feeds a death payoff of the robot's (a "dies" /
//! "leaves the battlefield" / sacrificed trigger whose card filter matches it —
//! Atla Palani's Eggs do, its other creatures do not). Anything else keeps the
//! action (fail open).

use engine::game::casting::can_pay_cost_after_auto_tap;
use engine::game::filter::{matches_target_filter, FilterContext};
use engine::types::ability::{AbilityCost, Effect, TargetFilter};
use engine::types::actions::GameAction;
use engine::types::game_state::WaitingFor;
use engine::types::identifiers::ObjectId;
use engine::types::triggers::TriggerMode;
use engine::types::zones::Zone;

use super::GateDecision;
use crate::policies::context::PolicyContext;

/// `ActivateAbility` of a sacrifice-cost mana ability the pending payment does
/// not need.
pub(super) fn assess_needless_sacrifice_for_mana(ctx: &PolicyContext<'_>) -> Option<GateDecision> {
    let WaitingFor::ManaPayment { player, .. } = &ctx.decision.waiting_for else {
        return None;
    };
    if *player != ctx.ai_player {
        return None;
    }
    let GameAction::ActivateAbility {
        source_id,
        ability_index,
        ..
    } = &ctx.candidate.action
    else {
        return None;
    };
    let source = ctx.state.objects.get(source_id)?;
    let ability = source.abilities.get(*ability_index)?;
    if !matches!(*ability.effect, Effect::Mana { .. }) {
        return None;
    }
    let mut filters = Vec::new();
    sacrificed_filters(ability.cost.as_ref()?, &mut filters);
    if filters.is_empty() {
        return None;
    }
    if sacrifice_feeds_death_payoff(ctx, source.id, &filters) {
        return None;
    }
    let pending = ctx.state.pending_cast.as_deref()?;
    if !can_pay_cost_after_auto_tap(ctx.state, *player, pending.object_id, &pending.cost) {
        return None;
    }
    if std::env::var_os("MF_GATE_TRACE").is_some() {
        eprintln!(
            "mf_flags_1125c reject {:?}: the pending cost is payable without the sacrifice",
            source.name
        );
    }
    Some(GateDecision::Reject)
}

/// The filters of every permanent `cost` sacrifices other than the source itself
/// (Treasure, Lotus Petal and the like sacrifice themselves: that is the card's
/// whole job).
fn sacrificed_filters<'a>(cost: &'a AbilityCost, out: &mut Vec<&'a TargetFilter>) {
    match cost {
        AbilityCost::Composite { costs } => {
            for cost in costs {
                sacrificed_filters(cost, out);
            }
        }
        AbilityCost::Sacrifice(sacrifice) if !matches!(sacrifice.target, TargetFilter::SelfRef) => {
            out.push(&sacrifice.target)
        }
        _ => {}
    }
}

/// Would one of the permanents the cost may sacrifice fire a death payoff the
/// robot controls (Blood Artist, Zulaport Cutthroat, Atla Palani's Eggs)? A
/// trigger out of the battlefield, into a graveyard, or on a sacrifice counts
/// when its card filter is absent or matches the candidate — an over-broad answer
/// only makes the gate stand down.
fn sacrifice_feeds_death_payoff(
    ctx: &PolicyContext<'_>,
    source: ObjectId,
    filters: &[&TargetFilter],
) -> bool {
    let cost_ctx = FilterContext::from_source(ctx.state, source);
    let candidates: Vec<ObjectId> = ctx
        .state
        .battlefield
        .iter()
        .copied()
        .filter(|id| {
            ctx.state
                .objects
                .get(id)
                .is_some_and(|object| object.controller == ctx.ai_player)
                && filters
                    .iter()
                    .any(|filter| matches_target_filter(ctx.state, *id, filter, &cost_ctx))
        })
        .collect();
    if candidates.is_empty() {
        return true;
    }
    ctx.state.battlefield.iter().any(|holder| {
        let Some(object) = ctx.state.objects.get(holder) else {
            return false;
        };
        if object.controller != ctx.ai_player {
            return false;
        }
        object.trigger_definitions.iter_unchecked().any(|entry| {
            let trigger = &entry.definition;
            let leaves = match trigger.mode {
                TriggerMode::ChangesZone | TriggerMode::ChangesZoneAll => {
                    trigger.origin == Some(Zone::Battlefield)
                        || trigger.origin_zones.contains(&Zone::Battlefield)
                        || trigger.destination == Some(Zone::Graveyard)
                }
                TriggerMode::Sacrificed => true,
                _ => false,
            };
            if !leaves {
                return false;
            }
            let Some(filter) = trigger.valid_card.as_ref() else {
                return true;
            };
            let holder_ctx = FilterContext::from_source(ctx.state, *holder);
            candidates
                .iter()
                .any(|id| matches_target_filter(ctx.state, *id, filter, &holder_ctx))
        })
    })
}
