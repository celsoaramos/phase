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
//! rejects the cast only when it can PROVE the spell is such a PROTECTIVE trick and
//! the combat window still comes: an instant whose whole resolution is non-negative
//! P/T, an after-blocks keyword (`keyword_settles_after_blocks`; never haste or
//! evasion) or a granted trigger, until end of turn, with at least one protective
//! piece (a granted trigger, hexproof, shroud or indestructible); the AI's own
//! precombat main phase; empty stack; no combat skip armed this turn; and no cast
//! trigger (prowess, "whenever you cast") on the AI's battlefield. A pure pump
//! (Coordinated Assault) stays with `EffectTiming`'s soft trick score, as upstream
//! pins it. Anything else keeps the action (fail open).

use engine::game::functioning_abilities::battlefield_active_triggers;
use engine::game::max_x_value;
use engine::types::ability::{
    Comparator, ContinuousModification, Duration, Effect, FilterProp, PtValue, TargetFilter,
    TypeFilter,
};
use engine::types::actions::GameAction;
use engine::types::card_type::CoreType;
use engine::types::game_state::{GameState, WaitingFor};
use engine::types::keywords::{Keyword, KeywordKind};
use engine::types::mana::{ManaCost, ManaCostShard};
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
    if effects.is_empty()
        || !effects.iter().all(|effect| is_combat_trick_effect(effect))
        || !effects.iter().any(|effect| is_protective_effect(effect))
    {
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

/// A grant that answers removal: a granted trigger (Supernatural Stamina's return on
/// death) or hexproof/shroud/indestructible. Holding it is what makes the precombat
/// cast a loss, not just a tell.
fn is_protective_effect(effect: &Effect) -> bool {
    let Effect::GenericEffect {
        static_abilities, ..
    } = effect
    else {
        return false;
    };
    static_abilities.iter().any(|stat| {
        stat.modifications.iter().any(|m| match m {
            ContinuousModification::GrantTrigger { .. } => true,
            ContinuousModification::AddKeyword { keyword } => matches!(
                keyword,
                Keyword::Hexproof
                    | Keyword::HexproofFrom(_)
                    | Keyword::Shroud
                    | Keyword::Indestructible
            ),
            _ => false,
        })
    })
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
pub(super) fn controls_cast_trigger(state: &GameState, player: PlayerId) -> bool {
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

/// Tutor for "a creature card with mana value X or less" cast when the only X the AI
/// can pay is 0 and its library holds no creature card with mana value 0 (MagicFinder
/// flag f909ba45, 22/11/2026: Chord of Calling in the robot's own upkeep with three
/// lands and no creature to convoke; the search found nothing and spent the turn).
/// CR 107.3 + CR 601.2b: X is announced on cast; at X=0 the filter reads "mana value
/// 0 or less". `x_cast_gate` does not see it because X lives in the search FILTER, not
/// in an amount. Narrow shape only: the whole resolution is the search, the move from
/// the library and the shuffle; the filter is exactly "creature card, mana value ≤ X".
/// Anything else keeps the action (fail open).
pub(super) fn assess_zero_x_creature_search(ctx: &PolicyContext<'_>) -> Option<GateDecision> {
    let GameAction::CastSpell { object_id, .. } = &ctx.candidate.action else {
        return None;
    };
    if !matches!(ctx.decision.waiting_for, WaitingFor::Priority { .. }) {
        return None;
    }
    let object = ctx.state.objects.get(object_id)?;
    let cost = &object.mana_cost;
    if !matches!(cost, ManaCost::Cost { shards, .. } if shards.iter().any(|s| matches!(s, ManaCostShard::X)))
    {
        return None;
    }
    let effects = ctx.effects();
    let mut searches = 0;
    for effect in &effects {
        match effect {
            Effect::SearchLibrary { filter, .. } => {
                if !is_creature_mv_le_x(filter) {
                    return None;
                }
                searches += 1;
            }
            Effect::ChangeZone { .. } | Effect::Shuffle { .. } => {}
            _ => return None,
        }
    }
    if searches != 1 {
        return None;
    }
    if max_x_value(ctx.state, ctx.ai_player, cost, Some(*object_id)) != 0 {
        return None;
    }
    let player = ctx.state.players.iter().find(|p| p.id == ctx.ai_player)?;
    let finds_something = player.library.iter().any(|id| {
        ctx.state.objects.get(id).is_some_and(|card| {
            card.card_types.core_types.contains(&CoreType::Creature)
                && card.mana_cost.mana_value() == 0
        })
    });
    if finds_something {
        return None;
    }
    if std::env::var_os("MF_GATE_TRACE").is_some() {
        eprintln!(
            "mf_flags_1124 reject zero-X creature search {:?}: nothing with mana value 0",
            object.name
        );
    }
    Some(GateDecision::Reject)
}

/// "a creature card with mana value X or less", and nothing else in the filter.
fn is_creature_mv_le_x(filter: &TargetFilter) -> bool {
    let TargetFilter::Typed(typed) = filter else {
        return false;
    };
    matches!(typed.type_filters.as_slice(), [TypeFilter::Creature])
        && matches!(
            typed.properties.as_slice(),
            [FilterProp::Cmc { comparator: Comparator::LE, value }] if value.contains_x()
        )
}
