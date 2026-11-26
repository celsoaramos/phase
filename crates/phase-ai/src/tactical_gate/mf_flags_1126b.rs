//! Two casts that do nothing (MagicFinder "jogada ruim do robô", 26/11/2026).
//!
//! - e2cd4180: in its own combat damage step, with nothing of the opponent's on
//!   the stack, the robot cracked two Treasures for Heroic Intervention
//!   ("Permanents you control gain hexproof and indestructible until end of
//!   turn."). CR 702.11b + CR 702.12b + CR 611.2a: hexproof and indestructible
//!   until end of turn answer removal or lethal damage that is coming; with no
//!   opponent spell or ability on the stack and no combat damage still to be
//!   dealt, the protection lapses at cleanup having stopped nothing, and the card
//!   that would answer the next removal is gone.
//! - dcaaede3: with an opponent who controlled one land and no other permanent,
//!   the robot cast Seam Rip ("When this enchantment enters, exile target nonland
//!   permanent an opponent controls with mana value 2 or less until this
//!   enchantment leaves the battlefield."). CR 603.3d: a triggered ability with
//!   no legal target is removed from the stack; an enchantment whose only ability
//!   is that trigger does nothing at all.
//!
//! Both gates reject only what they can PROVE and fail open otherwise.

use engine::game::players::is_opponent;
use engine::game::targeting::find_legal_targets;
use engine::types::ability::{ContinuousModification, Duration, Effect};
use engine::types::actions::GameAction;
use engine::types::card_type::CoreType;
use engine::types::game_state::WaitingFor;
use engine::types::keywords::Keyword;
use engine::types::phase::Phase;
use engine::types::triggers::TriggerMode;
use engine::types::zones::Zone;

use super::GateDecision;
use crate::policies::context::PolicyContext;

/// `CastSpell` of a pure protection instant with nothing to protect against.
pub(super) fn assess_idle_protection(ctx: &PolicyContext<'_>) -> Option<GateDecision> {
    let GameAction::CastSpell { .. } = &ctx.candidate.action else {
        return None;
    };
    if !matches!(ctx.decision.waiting_for, WaitingFor::Priority { .. }) {
        return None;
    }
    let facts = ctx.cast_facts()?;
    let types = &facts.object.card_types.core_types;
    if !types.contains(&CoreType::Instant) || types.contains(&CoreType::Creature) {
        return None;
    }
    let effects = ctx.effects();
    if effects.is_empty() || !effects.iter().all(|effect| is_protection_only(effect)) {
        return None;
    }
    // An opponent's spell or ability on the stack is what the protection answers.
    let threat_on_stack = ctx
        .state
        .stack
        .iter()
        .any(|entry| is_opponent(ctx.state, ctx.ai_player, entry.controller));
    if threat_on_stack || combat_damage_still_ahead(ctx) {
        return None;
    }
    if super::mf_flags_1124::controls_cast_trigger(ctx.state, ctx.ai_player) {
        return None;
    }
    if std::env::var_os("MF_GATE_TRACE").is_some() {
        eprintln!(
            "mf_flags_1126b reject idle protection {:?}: nothing to protect against",
            facts.object.name
        );
    }
    Some(GateDecision::Reject)
}

/// CR 510.1: combat damage is still to be dealt while a combat exists and the
/// combat damage step has not finished. Inside the combat damage step only a
/// first-strike creature (CR 510.4) leaves a second damage step to protect from.
fn combat_damage_still_ahead(ctx: &PolicyContext<'_>) -> bool {
    let Some(combat) = ctx.state.combat.as_ref() else {
        return false;
    };
    match ctx.state.phase {
        Phase::BeginCombat | Phase::DeclareAttackers | Phase::DeclareBlockers => true,
        Phase::CombatDamage => combat
            .attackers
            .iter()
            .map(|attacker| attacker.object_id)
            .chain(combat.blocker_assignments.values().flatten().copied())
            .any(|id| {
                ctx.state.objects.get(&id).is_some_and(|o| {
                    o.has_keyword(&Keyword::FirstStrike) || o.has_keyword(&Keyword::DoubleStrike)
                })
            }),
        _ => false,
    }
}

/// Until-end-of-turn grant of hexproof/shroud/indestructible and nothing else.
fn is_protection_only(effect: &Effect) -> bool {
    let Effect::GenericEffect {
        static_abilities,
        duration: Some(Duration::UntilEndOfTurn),
        ..
    } = effect
    else {
        return false;
    };
    !static_abilities.is_empty()
        && static_abilities.iter().all(|stat| {
            !stat.modifications.is_empty()
                && stat.modifications.iter().all(|m| {
                    matches!(
                        m,
                        ContinuousModification::AddKeyword {
                            keyword: Keyword::Hexproof
                                | Keyword::HexproofFrom(_)
                                | Keyword::Shroud
                                | Keyword::Indestructible
                        }
                    )
                })
        })
}

/// `CastSpell` of a noncreature permanent whose only ability is an enters trigger
/// that has no legal target now.
pub(super) fn assess_targetless_enters_permanent(ctx: &PolicyContext<'_>) -> Option<GateDecision> {
    let GameAction::CastSpell { .. } = &ctx.candidate.action else {
        return None;
    };
    let facts = ctx.cast_facts()?;
    let object = facts.object;
    let types = &object.card_types.core_types;
    let is_permanent = types.iter().any(|t| {
        matches!(
            t,
            CoreType::Artifact | CoreType::Enchantment | CoreType::Battle
        )
    });
    if !is_permanent
        || types.iter().any(|t| {
            matches!(
                t,
                CoreType::Creature | CoreType::Planeswalker | CoreType::Land
            )
        })
    {
        return None;
    }
    // Anything besides the enters trigger is value this gate cannot price.
    if !object.abilities.is_empty()
        || !object.keywords.is_empty()
        || object.static_definitions.iter_unchecked().next().is_some()
        || object
            .replacement_definitions
            .iter_unchecked()
            .next()
            .is_some()
    {
        return None;
    }
    let mut triggers = object.trigger_definitions.iter_unchecked().peekable();
    triggers.peek()?;
    for entry in triggers {
        let trigger = entry.definition();
        if trigger.mode != TriggerMode::ChangesZone
            || trigger.destination != Some(Zone::Battlefield)
            || !matches!(
                trigger.valid_card,
                Some(engine::types::ability::TargetFilter::SelfRef)
            )
        {
            return None;
        }
        let execute = trigger.execute.as_ref()?;
        if execute.sub_ability.is_some() || execute.optional_targeting {
            return None;
        }
        let filter = execute.effect.target_filter()?;
        if !find_legal_targets(ctx.state, filter, ctx.ai_player, object.id).is_empty() {
            return None;
        }
    }
    if super::mf_flags_1124::controls_cast_trigger(ctx.state, ctx.ai_player) {
        return None;
    }
    if std::env::var_os("MF_GATE_TRACE").is_some() {
        eprintln!(
            "mf_flags_1126b reject targetless enters permanent {:?}: no legal target",
            object.name
        );
    }
    Some(GateDecision::Reject)
}
