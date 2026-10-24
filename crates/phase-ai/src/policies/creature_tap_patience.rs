//! Creature tap-ability patience (Steel Overseer class).
//!
//! Report (MagicFinder, 2026-10-18): "the bot never leaves its Steel Overseer
//! untapped — it taps it for the counter every turn, so it is tapped the whole
//! time and never blocks." The log confirms the shape: the AI activated
//! "{T}: Put a +1/+1 counter on each artifact creature you control" in its OWN
//! draw step and beginning of combat, with no other creature to attack with,
//! and the Overseer stayed tapped through the opponent's whole turn (CR 502.3:
//! it untaps only in its controller's next untap step) while three creatures
//! attacked into an empty board.
//!
//! The rules make the patient line strictly available. A `{T}` activated
//! ability with no timing restriction can be activated whenever its controller
//! has priority (CR 602.2, CR 117.1b), and the AI receives priority in each
//! step of the opponent's turn (CR 117.3b, CR 117.4). Activated after the
//! opponent's combat — end of combat, the second main phase or the end step —
//! the counters land before the AI's next turn exactly as early as they would
//! from the AI's own upkeep, and the creature untaps in the AI's untap step
//! anyway. Waiting gives up nothing the effect does; activating early gives up
//! the creature as a blocker for the opponent's entire turn.
//!
//! # The class, never a card
//!
//! * The candidate is an `ActivateAbility` of an untapped CREATURE the AI
//!   controls, whose whole cost is `{T}` (`tap_only_cost`): no mana, so the
//!   window carries no mana-efficiency question, and no sacrifice.
//! * The ability has no activation restriction and no condition — any of
//!   those makes "later" a different question (CR 602.5b), so the policy
//!   stands down.
//! * Every effect in the chain is DURABLE: +1/+1 counters, card draw, scry,
//!   life gain. Their value does not depend on which window they resolve in.
//!   Anything window-bound (a pump until end of turn, "can't block this turn",
//!   damage aimed at a creature that may change) is out of scope.
//!
//! # Windows
//!
//! * AI's own turn → hold, except before attackers are declared when the
//!   counters reach ANOTHER creature that can attack this turn: there the
//!   activation buys a bigger attack now, and the trade is a judgement the
//!   policy does not make.
//! * Opponent's turn, before their combat is over → hold while the creature
//!   can still be declared as a blocker (CR 509.1a). Once blockers are
//!   declared, or no attacker was declared, the creature's defensive job for
//!   the turn is done (CR 506.4 — a tapped blocker stays a blocker), so the
//!   window is free.
//! * Opponent's turn after combat → free; this is the window the hold saves
//!   the activation for.
//!
//! An opponent with no creature on the battlefield has nothing for the
//! creature to block, so the policy stands down there too.
//!
//! # Why `Reject` and not a penalty
//!
//! A priority decision is ranked by search with the policy score weighted at
//! 0.1 (`search::choose_action_with_session`), and the board evaluation sees
//! the counter immediately while the lost block lies beyond its horizon (the
//! opponent's next combat). A soft penalty therefore loses to the counter
//! every time — which is the reported behaviour. The hold is a dominated-line
//! veto rather than a taste: the same activation stays available, for the
//! same effect, in a later window of the same round.

use engine::game::combat::has_summoning_sickness;
use engine::game::filter::{matches_target_filter, FilterContext};
use engine::game::mana_abilities;
use engine::game::players;
use engine::types::ability::{AbilityCost, AbilityDefinition, Effect, TargetFilter};
use engine::types::actions::GameAction;
use engine::types::card_type::CoreType;
use engine::types::counter::CounterType;
use engine::types::game_state::{GameState, WaitingFor};
use engine::types::identifiers::ObjectId;
use engine::types::keywords::Keyword;
use engine::types::phase::Phase;
use engine::types::player::PlayerId;

use super::context::PolicyContext;
use super::registry::{DecisionKind, PolicyId, PolicyReason, PolicyVerdict, TacticalPolicy};
use crate::features::DeckFeatures;

pub struct CreatureTapPatiencePolicy;

impl TacticalPolicy for CreatureTapPatiencePolicy {
    fn id(&self) -> PolicyId {
        PolicyId::CreatureTapPatience
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
        // The hold is a property of the window and of the ability's shape, not
        // of the deck. `verdict` exits on a card-local shape check first.
        // activation-constant: window/shape check, deck-independent.
        Some(1.0)
    }

    fn verdict(&self, ctx: &PolicyContext<'_>) -> PolicyVerdict {
        let neutral = |kind: &'static str| PolicyVerdict::neutral(PolicyReason::new(kind));

        let GameAction::ActivateAbility {
            source_id,
            ability_index,
            ..
        } = &ctx.candidate.action
        else {
            return neutral("creature_tap_patience_na");
        };
        let state = ctx.state;
        let Some(source) = state.objects.get(source_id) else {
            return neutral("creature_tap_patience_na");
        };
        let Some(ability) = source.abilities.get(*ability_index) else {
            return neutral("creature_tap_patience_na");
        };

        // 1. Card-local shape: a {T}-only, unrestricted, durable ability.
        if !ability_is_patient_tap(ability) {
            return neutral("creature_tap_patience_na");
        }

        // 2. The source must be an untapped creature the AI controls — the
        //    thing being protected is its availability as a blocker.
        if source.controller != ctx.ai_player
            || source.tapped
            || !source.card_types.core_types.contains(&CoreType::Creature)
        {
            return neutral("creature_tap_patience_na");
        }

        // 3. Only quiet priority windows. With anything on the stack the
        //    activation may be a response, which this policy does not judge.
        if !matches!(state.waiting_for, WaitingFor::Priority { player } if player == ctx.ai_player)
            || !state.stack.is_empty()
        {
            return neutral("creature_tap_patience_not_quiet");
        }

        // 4. Nothing to block: holding buys nothing.
        if !opponent_controls_creature(state, ctx.ai_player) {
            return neutral("creature_tap_patience_no_attackers");
        }

        if state.active_player == ctx.ai_player {
            if attack_still_ahead(state.phase)
                && counters_reach_another_attacker(state, ctx.ai_player, *source_id, ability)
            {
                return neutral("creature_tap_patience_pumps_attackers");
            }
            return PolicyVerdict::reject(
                PolicyReason::new("creature_tap_patience_own_turn")
                    .with_fact("phase", state.phase as i64),
            );
        }

        if blocker_still_needed(state) {
            return PolicyVerdict::reject(
                PolicyReason::new("creature_tap_patience_before_blocks")
                    .with_fact("phase", state.phase as i64),
            );
        }

        neutral("creature_tap_patience_free_window")
    }
}

/// `{T}`-only cost, no timing restriction, no condition, no modes, and every
/// effect in the chain durable.
fn ability_is_patient_tap(ability: &AbilityDefinition) -> bool {
    ability.cost.as_ref().is_some_and(tap_only_cost)
        && ability.activation_restrictions.is_empty()
        && ability.condition.is_none()
        && ability.ability_tag.is_none()
        && ability.mode_abilities.is_empty()
        && !mana_abilities::is_mana_ability(ability)
        && chain_is_durable(ability)
}

/// CR 602.1a: the cost is everything before the colon. Only `{T}` — a mana or
/// sacrifice component makes the window a different question.
fn tap_only_cost(cost: &AbilityCost) -> bool {
    match cost {
        AbilityCost::Tap => true,
        AbilityCost::Composite { costs } => !costs.is_empty() && costs.iter().all(tap_only_cost),
        _ => false,
    }
}

fn chain_is_durable(ability: &AbilityDefinition) -> bool {
    effect_is_durable(&ability.effect)
        && ability.duration.is_none()
        && ability.sub_ability.as_deref().is_none_or(chain_is_durable)
        && ability.else_ability.as_deref().is_none_or(chain_is_durable)
}

/// Effects whose value is the same whichever window they resolve in. A
/// counter (CR 122.1) and a drawn card stay; nothing here expires at cleanup.
fn effect_is_durable(effect: &Effect) -> bool {
    match effect {
        Effect::PutCounter { counter_type, .. } | Effect::PutCounterAll { counter_type, .. } => {
            matches!(counter_type, CounterType::Plus1Plus1)
        }
        Effect::Draw { .. } | Effect::Scry { .. } | Effect::GainLife { .. } => true,
        _ => false,
    }
}

fn opponent_controls_creature(state: &GameState, ai_player: PlayerId) -> bool {
    state.battlefield.iter().any(|id| {
        state.objects.get(id).is_some_and(|object| {
            players::is_opponent(state, ai_player, object.controller)
                && object.card_types.core_types.contains(&CoreType::Creature)
        })
    })
}

/// CR 508.1: attackers are declared in the declare attackers step, so a
/// counter placed before it can still change this turn's attack.
fn attack_still_ahead(phase: Phase) -> bool {
    matches!(
        phase,
        Phase::Upkeep | Phase::Draw | Phase::PreCombatMain | Phase::BeginCombat
    )
}

/// Does the ability put +1/+1 counters on a creature OTHER than the source
/// that can attack this turn (untapped, no summoning sickness — CR 302.6 —
/// and no defender, CR 702.3b)? Read through the engine's own filter so
/// "each artifact creature you control" is matched exactly.
fn counters_reach_another_attacker(
    state: &GameState,
    ai_player: PlayerId,
    source_id: ObjectId,
    ability: &AbilityDefinition,
) -> bool {
    let Some(filter) = counter_filter(ability) else {
        return false;
    };
    if matches!(filter, TargetFilter::SelfRef) {
        return false;
    }
    let filter_ctx = FilterContext::from_source(state, source_id);
    state.battlefield.iter().any(|&id| {
        id != source_id
            && state.objects.get(&id).is_some_and(|object| {
                object.controller == ai_player
                    && object.card_types.core_types.contains(&CoreType::Creature)
                    && !object.tapped
                    && !has_summoning_sickness(object)
                    && !object.has_keyword(&Keyword::Defender)
                    && matches_target_filter(state, id, filter, &filter_ctx)
            })
    })
}

fn counter_filter(ability: &AbilityDefinition) -> Option<&TargetFilter> {
    match &*ability.effect {
        Effect::PutCounter { target, .. } | Effect::PutCounterAll { target, .. } => Some(target),
        _ => ability.sub_ability.as_deref().and_then(counter_filter),
    }
}

/// CR 509.1a: during the opponent's turn the creature can still be declared
/// as a blocker until the declare blockers step. Before combat, or with
/// attackers declared but blocks not yet made, tapping it gives the block away.
fn blocker_still_needed(state: &GameState) -> bool {
    match state.phase {
        Phase::Upkeep | Phase::Draw | Phase::PreCombatMain | Phase::BeginCombat => true,
        Phase::DeclareAttackers => state
            .combat
            .as_ref()
            .is_some_and(|combat| !combat.attackers.is_empty()),
        _ => false,
    }
}
