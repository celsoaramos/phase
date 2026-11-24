//! Symmetric sweeper that costs the robot more than the opponent (MagicFinder
//! "jogada ruim do robô" flags ee2f4ebf and e347639b, 24/11/2026). In its own
//! beginning of combat, with 26 creatures (33 power) against an opponent at 24 life
//! behind 5 creatures (13 power), the robot activated Skirk Fire Marshal ("Tap five
//! untapped Goblins you control: ~ deals 10 damage to each creature and each
//! player"). The activation scored 120.947 against 120.923 for passing, so the
//! greedy pick took it, and the sweep killed the board that had lethal on the
//! attack.
//!
//! CR 120.3 + CR 704.5g: a "damage to each creature" effect destroys every creature
//! whose toughness it meets. This gate rejects the candidate only when it can PROVE
//! the sweep is a net loss: every effect the candidate reads is a fixed-amount
//! `DamageAll`, no opponent dies to the player damage, and the power of the robot's
//! own creatures it kills is greater than the power of the opponents' creatures it
//! kills. Indestructible and protected creatures count as survivors on both sides.
//! Anything else keeps the action (fail open).

use engine::game::filter::{matches_target_filter, FilterContext};
use engine::types::ability::{Effect, PlayerFilter, QuantityExpr};
use engine::types::actions::GameAction;
use engine::types::keywords::{Keyword, KeywordKind};
use engine::types::zones::Zone;

use super::GateDecision;
use crate::policies::context::PolicyContext;

/// `CastSpell` / `ActivateAbility` whose whole effect is a symmetric damage sweep.
pub(super) fn assess_self_sweeper(ctx: &PolicyContext<'_>) -> Option<GateDecision> {
    if !matches!(
        ctx.candidate.action,
        GameAction::CastSpell { .. } | GameAction::ActivateAbility { .. }
    ) {
        return None;
    }
    let source = ctx.source_object()?;
    let effects = ctx.effects();
    if effects.is_empty() {
        return None;
    }
    let mut own_lost = 0i32;
    let mut opp_lost = 0i32;
    for effect in effects {
        let Effect::DamageAll {
            amount: QuantityExpr::Fixed { value: amount },
            target,
            player_filter,
            ..
        } = effect
        else {
            return None;
        };
        let amount = *amount;
        if amount <= 0 {
            return None;
        }
        // CR 104.3a + CR 120.3: a sweep that also kills an opponent wins the game.
        if let Some(filter) = player_filter {
            let hits_opponents = matches!(filter, PlayerFilter::All | PlayerFilter::Opponent);
            if !hits_opponents {
                return None;
            }
            if ctx
                .state
                .players
                .iter()
                .any(|p| p.id != ctx.ai_player && !p.is_eliminated && p.life <= amount)
            {
                return None;
            }
        }
        let filter_ctx = FilterContext::from_source(ctx.state, source.id);
        for object in ctx.state.objects.values() {
            if object.zone != Zone::Battlefield || object.toughness.is_none() {
                continue;
            }
            if !matches_target_filter(ctx.state, object.id, target, &filter_ctx) {
                continue;
            }
            if object.has_keyword(&Keyword::Indestructible)
                || object.has_keyword_kind(KeywordKind::Protection)
            {
                continue;
            }
            let toughness = object.toughness.unwrap_or(0);
            if toughness - object.damage_marked as i32 > amount {
                continue;
            }
            let power = object.power.unwrap_or(0).max(0);
            if object.controller == ctx.ai_player {
                own_lost += power;
            } else {
                opp_lost += power;
            }
        }
    }
    if own_lost <= opp_lost {
        return None;
    }
    if std::env::var_os("MF_GATE_TRACE").is_some() {
        eprintln!(
            "mf_flags_1124b reject self sweeper {:?}: own power lost {own_lost} > opponents' {opp_lost}",
            source.name
        );
    }
    Some(GateDecision::Reject)
}
