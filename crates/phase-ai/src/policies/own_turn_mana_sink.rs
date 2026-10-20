//! Own-turn mana-sink patience (Silverquill Campus class).
//!
//! Report (MagicFinder, 2026-10-19, Commander, Kaalia of the Vast): in its own
//! upkeep the AI tapped four lands for Silverquill Campus's "{4}, {T}: Scry 1",
//! then reached its main phases with Kaalia in the command zone and passed
//! without casting her. The mana the scry ate was the mana the turn needed.
//!
//! A mana-costed activated ability with no timing restriction can be activated
//! whenever its controller has priority (CR 602.2, CR 117.1b), and the AI gets
//! priority in the opponent's end step (CR 117.3b, CR 513.1). There the lands
//! it taps untap in the AI's next untap step (CR 502.3) before they could have
//! paid for anything at sorcery speed, so a durable effect bought there costs
//! no mana the AI's own turn could use. Bought in the AI's own turn, the same
//! effect competes with every spell the AI could cast this turn — and a scry,
//! surveil or life gain gives nothing back THIS turn that the end-step window
//! would not also give before the AI's next draw.
//!
//! # The class, never a card
//!
//! * `ActivateAbility` of a permanent the AI controls, not a mana ability
//!   (CR 605.1a), whose cost spends mana and is otherwise only `{T}`.
//! * No activation restriction, no condition, no modes — "activate only as a
//!   sorcery" (CR 602.5d) or a condition makes "later" a different question.
//! * Every effect in the chain is a pure card-quality or life effect: scry,
//!   surveil, gain life. A draw is excluded on purpose: a card drawn in the
//!   AI's own turn can be played this turn, so that window is not dominated.
//!
//! # Windows
//!
//! * AI's own turn → hold.
//! * Opponent's turn → free; this is the window the hold saves it for.
//!
//! # Why `Reject` and not a penalty
//!
//! The scry shows up immediately in the AI's evaluation while the spell it
//! starves lies in a later decision of the same turn, so a soft penalty loses
//! to it under search (policy score weighted at 0.1). The hold is a dominated-
//! line veto: the same activation stays available, for the same effect, in a
//! later window of the same round.

use engine::game::mana_abilities;
use engine::types::ability::{AbilityCost, AbilityDefinition, Effect};
use engine::types::actions::GameAction;
use engine::types::game_state::{GameState, WaitingFor};
use engine::types::mana::ManaCost;
use engine::types::player::PlayerId;

use super::context::PolicyContext;
use super::registry::{DecisionKind, PolicyId, PolicyReason, PolicyVerdict, TacticalPolicy};
use crate::features::DeckFeatures;

pub struct OwnTurnManaSinkPolicy;

impl TacticalPolicy for OwnTurnManaSinkPolicy {
    fn id(&self) -> PolicyId {
        PolicyId::OwnTurnManaSink
    }

    fn decision_kinds(&self) -> &'static [DecisionKind] {
        &[DecisionKind::ActivateAbility]
    }

    fn activation(
        &self,
        _features: &DeckFeatures,
        _state: &GameState,
        _player: PlayerId,
    ) -> Option<f32> {
        // activation-constant: window/shape check, deck-independent.
        Some(1.0)
    }

    fn verdict(&self, ctx: &PolicyContext<'_>) -> PolicyVerdict {
        let neutral = |kind: &'static str| PolicyVerdict::neutral(PolicyReason::new(kind));

        let GameAction::ActivateAbility {
            source_id,
            ability_index,
        } = &ctx.candidate.action
        else {
            return neutral("own_turn_mana_sink_na");
        };
        let state = ctx.state;
        let Some(source) = state.objects.get(source_id) else {
            return neutral("own_turn_mana_sink_na");
        };
        let Some(ability) = source.abilities.get(*ability_index) else {
            return neutral("own_turn_mana_sink_na");
        };
        if source.controller != ctx.ai_player || !ability_is_deferrable_sink(ability) {
            return neutral("own_turn_mana_sink_na");
        }

        // Only quiet priority windows: with something on the stack the
        // activation may be a response, which this policy does not judge.
        if !matches!(state.waiting_for, WaitingFor::Priority { player } if player == ctx.ai_player)
            || !state.stack.is_empty()
        {
            return neutral("own_turn_mana_sink_not_quiet");
        }

        if state.active_player != ctx.ai_player {
            return neutral("own_turn_mana_sink_free_window");
        }

        PolicyVerdict::reject(
            PolicyReason::new("own_turn_mana_sink_hold").with_fact("phase", state.phase as i64),
        )
    }
}

fn ability_is_deferrable_sink(ability: &AbilityDefinition) -> bool {
    ability
        .cost
        .as_ref()
        .is_some_and(spends_mana_and_maybe_taps)
        && ability.activation_restrictions.is_empty()
        && ability.condition.is_none()
        && ability.ability_tag.is_none()
        && ability.mode_abilities.is_empty()
        && !mana_abilities::is_mana_ability(ability)
        && chain_is_deferrable(ability)
}

/// CR 602.1a: the cost is everything before the colon. Mana plus at most
/// `{T}` — sacrifice, discard or life make the window a different question.
fn spends_mana_and_maybe_taps(cost: &AbilityCost) -> bool {
    fn parts(cost: &AbilityCost) -> Option<bool> {
        match cost {
            AbilityCost::Mana {
                cost: ManaCost::Cost { shards, generic },
            } => Some(*generic > 0 || !shards.is_empty()),
            AbilityCost::Tap => Some(false),
            AbilityCost::Composite { costs } if !costs.is_empty() => {
                let mut spends = false;
                for part in costs {
                    spends |= parts(part)?;
                }
                Some(spends)
            }
            _ => None,
        }
    }
    parts(cost) == Some(true)
}

fn chain_is_deferrable(ability: &AbilityDefinition) -> bool {
    effect_is_deferrable(&ability.effect)
        && ability.duration.is_none()
        && ability
            .sub_ability
            .as_deref()
            .is_none_or(chain_is_deferrable)
        && ability
            .else_ability
            .as_deref()
            .is_none_or(chain_is_deferrable)
}

/// Effects that give nothing in the AI's own turn that the opponent's end
/// step would not also give before the AI's next draw.
fn effect_is_deferrable(effect: &Effect) -> bool {
    matches!(
        effect,
        Effect::Scry { .. } | Effect::Surveil { .. } | Effect::GainLife { .. }
    )
}
