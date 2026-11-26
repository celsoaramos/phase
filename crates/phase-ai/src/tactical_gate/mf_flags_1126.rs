//! Two classes from the MagicFinder "jogada ruim do robô" flags of 25/11/2026,
//! all four in the robot's mirror of a Sliver Commander deck.
//!
//! **A copy of a legend the robot already has** (f5d3e488, 25b8bcba). Rite of
//! Replication ("Create a token that's a copy of target creature") was aimed at
//! the robot's own commander, The Wandering Minstrel, and Shapesharer ("Target
//! Shapeshifter becomes a copy of target creature until your next turn") at its
//! Sliver Hivelord. CR 704.5j: when a player controls two or more legendary
//! permanents with the same name, that player chooses one and the rest go to the
//! graveyard — the robot sacrificed the token it had just made, then Shapesharer.
//! In a mirror the opponent's commander has the same name, so copying it loses the
//! same way. [`reject_legend_copy_target`] rejects such a target; the cast/
//! activation is rejected only when every legal copy source is one
//! ([`assess_legend_copy`]). Fails open on a copy that changes characteristics
//! ("except it isn't legendary"), a token handed to someone else, a recipient
//! slot (a legendary Shapeshifter becoming a copy), and any "legend rule doesn't
//! apply" static on the battlefield.
//!
//! **Budoka Gardener with no land in hand** (b8efa67b, 855aecdc). "{T}: You may
//! put a land card from your hand onto the battlefield. If you control ten or more
//! lands, flip this creature." was activated with only Slivers and Phyrexian Arena
//! in hand and four lands in play: it tapped a creature for nothing.
//! [`assess_futile_hand_land_drop`] rejects an activation whose whole chain is a
//! put-from-hand that no card in the robot's hand matches, plus flips whose
//! condition is false now (evaluated by the engine).

use engine::ai_support::current_target_selection_targets;
use engine::game::effects::evaluate_condition;
use engine::game::filter::{matches_target_filter, FilterContext};
use engine::game::functioning_abilities::static_kind_present;
use engine::game::targeting::find_legal_targets;
use engine::types::ability::{AbilityDefinition, CopyRecipient, Effect, TargetFilter, TargetRef};
use engine::types::actions::GameAction;
use engine::types::card_type::Supertype;
use engine::types::identifiers::ObjectId;
use engine::types::statics::StaticModeKind;
use engine::types::zones::Zone;

use super::GateDecision;
use crate::policies::context::PolicyContext;

fn trace(message: impl FnOnce() -> String) {
    if std::env::var_os("MF_GATE_TRACE").is_some() {
        eprintln!("mf_flags_1126 {}", message());
    }
}

/// The copy-source filters of the candidate's copy effects, with the recipient
/// filters to leave alone. `None` when the shape is not one this gate proves.
struct CopyShape<'a> {
    sources: Vec<&'a TargetFilter>,
    recipients: Vec<&'a TargetFilter>,
}

fn copy_shape<'a>(ctx: &PolicyContext<'a>) -> Option<CopyShape<'a>> {
    let effects = ctx.effects();
    let mut shape = CopyShape {
        sources: Vec::new(),
        recipients: Vec::new(),
    };
    for effect in effects {
        match effect {
            Effect::CopyTokenOf {
                target,
                owner,
                additional_modifications,
                ..
            } => {
                if !additional_modifications.is_empty()
                    || !matches!(owner, TargetFilter::Controller)
                {
                    return None;
                }
                shape.sources.push(target);
            }
            Effect::BecomeCopy {
                target,
                recipient,
                additional_modifications,
                ..
            } => {
                if !additional_modifications.is_empty() {
                    return None;
                }
                match recipient {
                    CopyRecipient::Source => {}
                    CopyRecipient::Target(filter) => shape.recipients.push(filter),
                    CopyRecipient::Untargeted(_) => return None,
                }
                shape.sources.push(target);
            }
            _ => {}
        }
    }
    if shape.sources.is_empty() {
        return None;
    }
    if static_kind_present(ctx.state, StaticModeKind::LegendRuleDoesntApply) {
        return None;
    }
    Some(shape)
}

/// Would a copy of `object_id`, controlled by the robot, meet a legendary
/// permanent of the same name the robot already controls (CR 704.5j)?
fn copy_meets_own_legend(ctx: &PolicyContext<'_>, object_id: ObjectId) -> bool {
    let Some(object) = ctx.state.objects.get(&object_id) else {
        return false;
    };
    if object.zone != Zone::Battlefield
        || !object.card_types.supertypes.contains(&Supertype::Legendary)
        || object.name.is_empty()
    {
        return false;
    }
    ctx.state.battlefield.iter().any(|id| {
        ctx.state.objects.get(id).is_some_and(|own| {
            own.controller == ctx.ai_player
                && own.name == object.name
                && own.card_types.supertypes.contains(&Supertype::Legendary)
        })
    })
}

fn is_recipient_candidate(
    ctx: &PolicyContext<'_>,
    shape: &CopyShape<'_>,
    source: ObjectId,
    object_id: ObjectId,
) -> bool {
    let filter_ctx = FilterContext::from_source(ctx.state, source);
    shape
        .recipients
        .iter()
        .any(|filter| matches_target_filter(ctx.state, object_id, filter, &filter_ctx))
}

/// `ChooseTarget`/`SelectTargets`: the chosen copy source is a legend the robot
/// would lose to the legend rule, while another legal target is not.
pub(super) fn reject_legend_copy_target(ctx: &PolicyContext<'_>, target: &TargetRef) -> bool {
    let TargetRef::Object(object_id) = target else {
        return false;
    };
    let Some(shape) = copy_shape(ctx) else {
        return false;
    };
    let Some(source) = ctx.source_object() else {
        return false;
    };
    if source.controller != ctx.ai_player
        || is_recipient_candidate(ctx, &shape, source.id, *object_id)
        || !copy_meets_own_legend(ctx, *object_id)
    {
        return false;
    }
    let Some(legal) = current_target_selection_targets(ctx.state) else {
        return false;
    };
    let alternative = legal.iter().any(|other| match other {
        TargetRef::Object(id) => *id != *object_id && !copy_meets_own_legend(ctx, *id),
        TargetRef::Player(_) => false,
    });
    if alternative {
        trace(|| {
            format!(
                "reject copy target {:?}: legend rule",
                ctx.state.objects[object_id].name
            )
        });
    }
    alternative
}

/// `CastSpell`/`ActivateAbility` of a copy whose every legal copy source is a
/// legend the robot would lose to the legend rule.
pub(super) fn assess_legend_copy(ctx: &PolicyContext<'_>) -> Option<GateDecision> {
    let source = match &ctx.candidate.action {
        GameAction::CastSpell { object_id, .. } => *object_id,
        GameAction::ActivateAbility { source_id, .. } => *source_id,
        _ => return None,
    };
    let object = ctx.state.objects.get(&source)?;
    if object.controller != ctx.ai_player || object.modal.is_some() {
        return None;
    }
    let shape = copy_shape(ctx)?;
    for filter in &shape.sources {
        let targets = find_legal_targets(ctx.state, filter, ctx.ai_player, source);
        if targets.is_empty() {
            return None;
        }
        let all_lose = targets.iter().all(|target| match target {
            TargetRef::Object(id) => {
                !is_recipient_candidate(ctx, &shape, source, *id) && copy_meets_own_legend(ctx, *id)
            }
            TargetRef::Player(_) => false,
        });
        if !all_lose {
            return None;
        }
    }
    trace(|| {
        format!(
            "reject {:?}: every copy source is a legend the robot has",
            object.name
        )
    });
    Some(GateDecision::Reject)
}

/// `ActivateAbility` whose chain only puts a card from the robot's hand onto the
/// battlefield with nothing in hand to put, plus flips whose condition is false.
pub(super) fn assess_futile_hand_land_drop(ctx: &PolicyContext<'_>) -> Option<GateDecision> {
    let GameAction::ActivateAbility {
        source_id,
        ability_index,
        ..
    } = &ctx.candidate.action
    else {
        return None;
    };
    let source = ctx.state.objects.get(source_id)?;
    if source.controller != ctx.ai_player {
        return None;
    }
    let ability = source.abilities.get(*ability_index)?;
    let mut puts_from_hand = false;
    let mut node: Option<&AbilityDefinition> = Some(ability);
    while let Some(definition) = node {
        if definition.else_ability.is_some() || !definition.mode_abilities.is_empty() {
            return None;
        }
        match definition.effect.as_ref() {
            Effect::ChangeZone {
                origin: Some(Zone::Hand),
                destination: Zone::Battlefield,
                target,
                ..
            } if definition.condition.is_none() => {
                let filter_ctx = FilterContext::from_source(ctx.state, source.id);
                let hand = &ctx
                    .state
                    .players
                    .iter()
                    .find(|p| p.id == ctx.ai_player)?
                    .hand;
                if hand
                    .iter()
                    .any(|id| matches_target_filter(ctx.state, *id, target, &filter_ctx))
                {
                    return None;
                }
                puts_from_hand = true;
            }
            Effect::FlipPermanent { .. } => {
                let condition = definition.condition.as_ref()?;
                let resolved = engine::game::ability_utils::build_resolved_from_def_with_targets(
                    definition,
                    source.id,
                    source.controller,
                    Vec::new(),
                );
                if evaluate_condition(condition, ctx.state, &resolved) {
                    return None;
                }
            }
            _ => return None,
        }
        node = definition.sub_ability.as_deref();
    }
    if !puts_from_hand {
        return None;
    }
    trace(|| {
        format!(
            "reject {:?}: nothing in hand to put onto the battlefield",
            source.name
        )
    });
    Some(GateDecision::Reject)
}
