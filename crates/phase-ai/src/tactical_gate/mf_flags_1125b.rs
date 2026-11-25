//! Harmful Aura with no opponent to enchant (MagicFinder "jogada ruim do robô"
//! 7dee6cac, 17/11/2026). With no creature on the opponent's side, the robot cast
//! Aether Meltdown ("Enchant creature. When Aether Meltdown enters, you get {E}{E}.
//! Enchanted creature gets -4/-0.") on its own creature, only for the two energy.
//!
//! CR 303.4a + CR 303.4b: an Aura spell targets the object it will enchant. A
//! harmful Aura (`aura_polarity` = Harmful: a P/T penalty, "can't attack/block")
//! on the robot's own permanent hurts the robot. `anti_self_harm` already charges
//! this as a soft penalty, but only when the Aura has NO other effect: Aether
//! Meltdown's enters trigger made the effect list non-empty and the penalty never
//! applied, and under search a soft penalty does not hold the cast anyway. This
//! gate rejects the cast only when it can PROVE it: the spell is an Aura with
//! harmful polarity, the engine's legal targets for its Enchant filter hold no
//! permanent an opponent controls, and the robot has no cast trigger (prowess,
//! "whenever you cast"). Anything else keeps the action (fail open).

use engine::game::players::is_opponent;
use engine::game::targeting::find_legal_targets;
use engine::types::ability::TargetRef;
use engine::types::actions::GameAction;
use engine::types::keywords::Keyword;

use super::GateDecision;
use crate::policies::context::PolicyContext;
use crate::policies::effect_classify::{aura_polarity, EffectPolarity};

/// `CastSpell` of a harmful Aura with only the robot's own permanents to enchant.
pub(super) fn assess_self_harmful_aura(ctx: &PolicyContext<'_>) -> Option<GateDecision> {
    let GameAction::CastSpell { .. } = &ctx.candidate.action else {
        return None;
    };
    let facts = ctx.cast_facts()?;
    let aura = facts.object;
    if !aura.card_types.subtypes.iter().any(|s| s == "Aura") {
        return None;
    }
    if !matches!(aura_polarity(aura), EffectPolarity::Harmful) {
        return None;
    }
    let filter = aura.keywords.iter().find_map(|keyword| match keyword {
        Keyword::Enchant(filter) => Some(filter),
        _ => None,
    })?;
    let opponent_target = find_legal_targets(ctx.state, filter, ctx.ai_player, aura.id)
        .into_iter()
        .any(|target| match target {
            TargetRef::Object(id) => ctx
                .state
                .objects
                .get(&id)
                .is_some_and(|object| is_opponent(ctx.state, ctx.ai_player, object.controller)),
            TargetRef::Player(player) => is_opponent(ctx.state, ctx.ai_player, player),
        });
    if opponent_target {
        return None;
    }
    if super::mf_flags_1124::controls_cast_trigger(ctx.state, ctx.ai_player) {
        return None;
    }
    if std::env::var_os("MF_GATE_TRACE").is_some() {
        eprintln!(
            "mf_flags_1125b reject harmful aura {:?}: no opponent permanent to enchant",
            aura.name
        );
    }
    Some(GateDecision::Reject)
}
