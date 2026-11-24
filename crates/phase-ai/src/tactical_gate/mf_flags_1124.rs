//! Combat trick cast before combat (MagicFinder "jogada ruim do robô" flag 76e88b36,
//! 24/11/2026). In its own first main phase, with an empty stack, the robot cast
//! Supernatural Stamina ("Until end of turn, target creature gets +2/+0 and gains
//! 'When this creature dies, return it to the battlefield tapped under its owner's
//! control.'") on its Voldaren Bloodcaster. The cast scored -10.93 against -10.97
//! for passing, so the greedy pick took it.
//!
//! CR 514.2 + CR 611.2a: an until-end-of-turn pump or grant does the same work cast
//! during combat — after attackers (CR 508) or after blockers (CR 509) — and the
//! death grant is a response to removal. Cast in the precombat main phase it shows
//! the trick before the opponent blocks and throws away the protection. This gate
//! rejects the cast only when it can PROVE the spell is such a trick and the combat
//! window still comes: an instant whose whole resolution is non-negative P/T, an
//! after-blocks keyword (`keyword_settles_after_blocks`; never haste or evasion) or a
//! granted trigger, until end of turn; the AI's own precombat main phase; empty
//! stack; no combat skip armed this turn; and no cast trigger (prowess, "whenever you
//! cast") on the AI's battlefield. Anything else keeps the action (fail open).

use engine::game::functioning_abilities::battlefield_active_triggers;
use engine::types::ability::{ContinuousModification, Duration, Effect, PtValue};
use engine::types::actions::GameAction;
use engine::types::card_type::CoreType;
use engine::types::game_state::{GameState, WaitingFor};
use engine::types::keywords::{Keyword, KeywordKind};
use engine::types::phase::Phase;
use engine::types::player::PlayerId;
use engine::types::triggers::TriggerMode;

use super::GateDecision;
use crate::policies::context::PolicyContext;

/// `CastSpell` announced from priority. `Some(Reject)` removes the candidate.
pub(super) fn assess_precombat_trick(ctx: &PolicyContext<'_>) -> Option<GateDecision> {
    let GameAction::CastSpell { .. } = &ctx.candidate.action else {
        return None;
    };
    if !matches!(ctx.decision.waiting_for, WaitingFor::Priority { .. })
        || ctx.state.phase != Phase::PreCombatMain
        || ctx.state.active_player != ctx.ai_player
        || !ctx.state.stack.is_empty()
        || ctx.state.combat.is_some()
        || ctx
            .state
            .combat_phase_skip_next_turn
            .iter()
            .any(|s| s.active)
    {
        return None;
    }
    let facts = ctx.cast_facts()?;
    let types = &facts.object.card_types.core_types;
    if !types.contains(&CoreType::Instant)
        || types.contains(&CoreType::Creature)
        || types.contains(&CoreType::Sorcery)
    {
        return None;
    }
    let effects = ctx.effects();
    if effects.is_empty() || !effects.iter().all(|effect| is_combat_trick_effect(effect)) {
        return None;
    }
    if controls_cast_trigger(ctx.state, ctx.ai_player) {
        return None;
    }
    if std::env::var_os("MF_GATE_TRACE").is_some() {
        eprintln!(
            "mf_flags_1124 reject precombat trick {:?}: combat window still ahead",
            facts.object.name
        );
    }
    Some(GateDecision::Reject)
}

/// An until-end-of-turn pump/grant whose value is settled in combat.
pub fn is_combat_trick_effect(effect: &Effect) -> bool {
    let non_negative = |v: &PtValue| !matches!(v, PtValue::Fixed(n) if *n < 0);
    match effect {
        // CR 611.2a: a single-target `Pump` lasts until end of turn.
        Effect::Pump {
            power, toughness, ..
        } => non_negative(power) && non_negative(toughness),
        Effect::GenericEffect {
            static_abilities,
            duration: Some(Duration::UntilEndOfTurn),
            ..
        } => {
            !static_abilities.is_empty()
                && static_abilities.iter().all(|stat| {
                    !stat.modifications.is_empty()
                        && stat.modifications.iter().all(|m| match m {
                            ContinuousModification::AddPower { value }
                            | ContinuousModification::AddToughness { value } => *value >= 0,
                            ContinuousModification::AddKeyword { keyword } => {
                                keyword_settles_after_blocks(keyword)
                            }
                            ContinuousModification::GrantTrigger { .. } => true,
                            _ => false,
                        })
                })
        }
        _ => false,
    }
}

/// Keywords whose value is settled after blockers are declared. Evasion and haste
/// are deliberately absent: they matter before attackers / blockers.
fn keyword_settles_after_blocks(keyword: &Keyword) -> bool {
    matches!(
        keyword,
        Keyword::Hexproof
            | Keyword::HexproofFrom(_)
            | Keyword::Shroud
            | Keyword::Indestructible
            | Keyword::FirstStrike
            | Keyword::DoubleStrike
            | Keyword::Deathtouch
            | Keyword::Lifelink
            | Keyword::Trample
    )
}

/// True when casting the spell itself pays something (prowess, "whenever you cast").
/// Fails OPEN: any cast/copy trigger the AI controls counts.
fn controls_cast_trigger(state: &GameState, player: PlayerId) -> bool {
    let prowess = state.battlefield.iter().any(|id| {
        state
            .objects
            .get(id)
            .is_some_and(|o| o.controller == player && o.has_keyword_kind(KeywordKind::Prowess))
    });
    prowess
        || battlefield_active_triggers(state).any(|(source, active)| {
            source.controller == player
                && matches!(
                    active.definition.mode,
                    TriggerMode::SpellCast
                        | TriggerMode::SpellCopy
                        | TriggerMode::SpellCastOrCopy
                        | TriggerMode::SpellAbilityCast
                        | TriggerMode::SpellAbilityCopy
                )
        })
}
