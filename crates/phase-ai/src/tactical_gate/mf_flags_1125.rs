//! Untap ability with nothing of the robot's to untap (MagicFinder "jogada ruim
//! do robô" e8ee3d01, 24/11/2026, and 3d498e2e, 17/11/2026). In its own
//! beginning of combat, with every creature it controlled already untapped, the
//! robot activated Wirewood Symbiote ("Return an Elf you control to its owner's
//! hand: Untap target creature."): it bounced an Elf to untap a creature that
//! was untapped already, or one of the opponent's. Magus of the Candelabra
//! ("{X}, {T}: Untap X target lands.") was activated the same way with every land
//! untapped.
//!
//! CR 701.26b: to untap a permanent is to turn it upright; untapping an untapped
//! permanent does nothing, and untapping an opponent's permanent only helps that
//! opponent. This gate rejects an activated ability only when it can PROVE the
//! untap is worthless to the robot: every effect is a single-target `Untap`, no
//! tapped permanent the robot controls matches the target filter, and no cost
//! returns a permanent of the robot's with a triggered ability (re-buying an
//! enters trigger is a real reason to activate Wirewood Symbiote). Anything else
//! keeps the action (fail open).

use engine::game::filter::{matches_target_filter, FilterContext};
use engine::types::ability::{AbilityCost, Effect, EffectScope, TapStateChange, TargetFilter};
use engine::types::actions::GameAction;
use engine::types::zones::Zone;

use super::GateDecision;
use crate::policies::context::PolicyContext;

/// `ActivateAbility` whose whole effect untaps a target the robot gains nothing from.
pub(super) fn assess_futile_untap(ctx: &PolicyContext<'_>) -> Option<GateDecision> {
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
    let effects = ctx.effects();
    if effects.is_empty() {
        return None;
    }
    let mut targets: Vec<&TargetFilter> = Vec::new();
    for effect in effects {
        let Effect::SetTapState {
            target,
            scope: EffectScope::Single,
            state: TapStateChange::Untap,
        } = effect
        else {
            return None;
        };
        targets.push(target);
    }
    if ability
        .cost
        .as_ref()
        .is_some_and(|cost| returns_triggering_permanent(ctx, source.id, cost))
    {
        return None;
    }
    let filter_ctx = FilterContext::from_source(ctx.state, source.id);
    let own_tapped_target = ctx.state.objects.values().any(|object| {
        object.zone == Zone::Battlefield
            && object.controller == ctx.ai_player
            && object.tapped
            && targets
                .iter()
                .any(|target| matches_target_filter(ctx.state, object.id, target, &filter_ctx))
    });
    if own_tapped_target {
        return None;
    }
    if std::env::var_os("MF_GATE_TRACE").is_some() {
        eprintln!(
            "mf_flags_1125 reject futile untap {:?}: no tapped permanent of the robot to untap",
            source.name
        );
    }
    Some(GateDecision::Reject)
}

/// Does `cost` return to hand a permanent of the robot's that has a triggered
/// ability (an enters trigger the activation re-buys)? Unknown shapes answer
/// `true` so the gate fails open.
fn returns_triggering_permanent(
    ctx: &PolicyContext<'_>,
    source: engine::types::identifiers::ObjectId,
    cost: &AbilityCost,
) -> bool {
    match cost {
        AbilityCost::Composite { costs } => costs
            .iter()
            .any(|cost| returns_triggering_permanent(ctx, source, cost)),
        AbilityCost::ReturnToHand { filter, .. } => {
            let filter_ctx = FilterContext::from_source(ctx.state, source);
            ctx.state.objects.values().any(|object| {
                object.zone == Zone::Battlefield
                    && object.controller == ctx.ai_player
                    && filter.as_ref().is_none_or(|filter| {
                        matches_target_filter(ctx.state, object.id, filter, &filter_ctx)
                    })
                    && !object.trigger_definitions.is_empty()
            })
        }
        _ => false,
    }
}
