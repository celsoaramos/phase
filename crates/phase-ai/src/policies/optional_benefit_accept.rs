use engine::types::ability::{Effect, EffectScope, ResolvedAbility, TapStateChange, TargetFilter};
use engine::types::actions::GameAction;
use engine::types::counter::CounterType;
use engine::types::game_state::{GameState, WaitingFor};
use engine::types::player::PlayerId;
use engine::types::resolution::ResolutionFrame;
use engine::types::zones::Zone;

use crate::features::DeckFeatures;

use super::context::PolicyContext;
use super::registry::{
    DecisionKind, PolicyId, PolicyReason, PolicyVerdict, TacticalPolicy, CRITICAL_MAX,
};

/// CR 603.5 + CR 608.2d: "you may [do something good for you]" on the AI's own
/// resolving ability.
///
/// The search scores both answers of an `OptionalEffectChoice` by projecting
/// the position, and for a large class of optional instructions the projection
/// does not see the effect at all — accept and decline come back with the SAME
/// score (measured on v0.110.0: Aether Vial's charge counter and its put-a-
/// creature tap, Brood Sliver's token, Land Tax), and the tie resolves to
/// `accept: false`, the first issued candidate. A bot running Aether Vial never
/// charged it.
///
/// Two verdicts, both only for the AI's own prompt:
/// * **Unless an opponent pays** (Rhystic Study, Mystic Remora): accepting costs
///   the AI nothing — the payment, if any, is the opponent's — so declining a
///   beneficial chain is rejected outright. The projection got this one wrong
///   the other way (Medium scored accept four points BELOW decline).
/// * Otherwise a bonus toward accept at the top of the CRITICAL band. With
///   search on, policy deltas enter at ×0.1 against the board evaluation, so
///   this is +1.5 of evaluation — still less than a card — and it has to be:
///   the choice is a softmax DRAW, and a +0.25 nudge (→ +0.025) left Vial
///   uncharged half the time. +1.5 is ~82% accept at Medium (temperature 1.0)
///   and ~99% at VeryHard (0.3), the level MagicFinder's Hard bot runs. A
///   search that does see the downside still outweighs it.
///
/// "Beneficial" is an allow-list over every link of the chain; anything the
/// list does not know (a cost, a sacrifice, a discard, a life payment, an
/// opponent-side effect) leaves the decision to the search.
pub struct OptionalBenefitAcceptPolicy;

impl TacticalPolicy for OptionalBenefitAcceptPolicy {
    fn id(&self) -> PolicyId {
        PolicyId::OptionalBenefitAccept
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
        // activation-constant: prompt gating lives in `verdict`.
        Some(1.0)
    }

    fn verdict(&self, ctx: &PolicyContext<'_>) -> PolicyVerdict {
        let neutral = || PolicyVerdict::neutral(PolicyReason::new("optional_benefit_na"));
        let WaitingFor::OptionalEffectChoice { player, .. } = &ctx.decision.waiting_for else {
            return neutral();
        };
        if *player != ctx.ai_player {
            return neutral();
        }
        let GameAction::DecideOptionalEffect { accept } = ctx.candidate.action else {
            return neutral();
        };
        let Some(ability) = parked_optional_ability(ctx.state) else {
            return neutral();
        };
        if ability.controller == ctx.ai_player {
            if let Some(verdict) = exile_keep_verdict(ctx.state, ctx.ai_player, ability, accept) {
                return verdict;
            }
        }
        if ability.controller != ctx.ai_player
            || !chain_is_beneficial(ctx.state, ctx.ai_player, ability)
        {
            return neutral();
        }
        let opponent_pays = ability.unless_pay.as_ref().is_some_and(|unless| {
            !matches!(
                unless.payer,
                TargetFilter::Controller | TargetFilter::SelfRef
            )
        });
        match (opponent_pays, accept) {
            (true, false) => {
                PolicyVerdict::reject(PolicyReason::new("optional_benefit_opponent_pays"))
            }
            (false, true) => PolicyVerdict::critical(
                CRITICAL_MAX,
                PolicyReason::new("optional_benefit_tiebreak"),
            ),
            _ => neutral(),
        }
    }
}

/// The ability parked by the engine while it waits on the optional decision.
fn parked_optional_ability(state: &GameState) -> Option<&ResolvedAbility> {
    state
        .resolution_stack
        .iter()
        .filter_map(|frame| match frame {
            ResolutionFrame::OptionalEffect(frame) => Some(frame.ability.as_ref()),
            _ => None,
        })
        .last()
}

/// CR 608.2c: "Exile the top card of your library. You may put that card into
/// your hand …" — the keep step of Tainted Pact's repeat process, and of every
/// exile-then-keep instruction. The parked link is a `ChangeZone` to hand over
/// `ParentTarget` with no declared origin, so the allow-list below does not see
/// it; the projection scores both answers alike and the tie went to DECLINE on
/// every iteration — the bot exiled its whole library (MagicFinder bad-play flag
/// of 2026-11-15, 90 cards).
///
/// Declining is rejected when the card on offer is a spell, when it is a land
/// the AI still needs (fewer than four lands on its battlefield), or when the
/// library is nearly gone. A land offered to an AI with lands and a deep
/// library is left to the search: digging one card further for a spell is a
/// fair line, and the next spell ends the dig.
fn exile_keep_verdict(
    state: &GameState,
    ai: PlayerId,
    ability: &ResolvedAbility,
    accept: bool,
) -> Option<PolicyVerdict> {
    let Effect::ChangeZone {
        origin: None,
        destination: Zone::Hand,
        target: TargetFilter::ParentTarget,
        enters_under: None,
        ..
    } = &ability.effect
    else {
        return None;
    };
    if ability.else_ability.is_some()
        || ability.sub_ability.is_some()
        || ability.unless_pay.is_some()
    {
        return None;
    }
    let mut cards = ability.targets.iter().filter_map(|t| match t {
        engine::types::ability::TargetRef::Object(id) => state.objects.get(id),
        engine::types::ability::TargetRef::Player(_) => None,
    });
    let card = cards.next()?;
    if cards.next().is_some() || card.owner != ai || card.zone != Zone::Exile {
        return None;
    }
    let is_land = card
        .card_types
        .core_types
        .contains(&engine::types::card_type::CoreType::Land);
    let own_lands = state
        .battlefield
        .iter()
        .filter_map(|id| state.objects.get(id))
        .filter(|o| {
            o.controller == ai
                && o.card_types
                    .core_types
                    .contains(&engine::types::card_type::CoreType::Land)
        })
        .count();
    let library = state
        .players
        .get(ai.0 as usize)
        .map_or(0, |p| p.library.len());
    let must_keep = !is_land || own_lands < 4 || library <= 10;
    if !must_keep {
        return None;
    }
    Some(if accept {
        PolicyVerdict::critical(CRITICAL_MAX, PolicyReason::new("exile_keep_accept"))
    } else {
        PolicyVerdict::reject(PolicyReason::new("exile_keep_decline_burns_library"))
    })
}

fn chain_is_beneficial(state: &GameState, ai: PlayerId, ability: &ResolvedAbility) -> bool {
    let mut link = Some(ability);
    while let Some(a) = link {
        if a.else_ability.is_some() || !effect_is_beneficial(state, ai, &a.effect) {
            return false;
        }
        link = a.sub_ability.as_deref();
    }
    true
}

/// The chain only runs for an ability the AI controls (`verdict`), so these
/// all name the AI: its controller, the source itself, and — for a "its
/// controller may" trigger (Brood Sliver) — the controller of the AI's own
/// creature that the trigger points at.
fn is_own(filter: &TargetFilter) -> bool {
    matches!(
        filter,
        TargetFilter::Controller | TargetFilter::SelfRef | TargetFilter::ParentTargetController
    )
}

fn effect_is_beneficial(state: &GameState, ai: PlayerId, effect: &Effect) -> bool {
    match effect {
        // CR 121.4 + CR 704.5b: drawing from an almost-empty library is the one
        // way a draw hurts — keep a margin and let the search decide there.
        Effect::Draw { target, .. } => {
            is_own(target)
                && state
                    .players
                    .get(ai.0 as usize)
                    .is_some_and(|p| p.library.len() > 3)
        }
        Effect::PutCounter {
            counter_type,
            target,
            ..
        } => !matches!(counter_type, CounterType::Minus1Minus1) && is_own(target),
        Effect::Token { owner, .. } => is_own(owner),
        Effect::GainLife { player, .. } => is_own(player),
        Effect::Scry { target, .. }
        | Effect::Surveil { target, .. }
        | Effect::Shuffle { target } => is_own(target),
        // Own cards coming into play or into hand (Aether Vial, Sun Titan, Land
        // Tax's search): never from the battlefield, never under another player.
        Effect::ChangeZone {
            origin,
            destination,
            enters_under,
            ..
        } => {
            matches!(
                origin,
                Some(Zone::Hand | Zone::Library | Zone::Graveyard | Zone::Exile)
            ) && matches!(destination, Zone::Battlefield | Zone::Hand)
                && enters_under.is_none()
        }
        Effect::SearchLibrary { target_player, .. } => target_player.as_ref().is_none_or(is_own),
        Effect::Investigate | Effect::Proliferate | Effect::Populate => true,
        Effect::Mana { .. } | Effect::GainEnergy { .. } => true,
        // CR 701.26b: "you may untap ~" (Blasting Station, Arcanis-style engines) —
        // untapping the source itself; measured declined 4 of 5 in AI × AI games.
        Effect::SetTapState {
            target: TargetFilter::SelfRef,
            scope: EffectScope::Single,
            state: TapStateChange::Untap,
        } => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn untap(target: TargetFilter, state: TapStateChange) -> Effect {
        Effect::SetTapState {
            target,
            scope: EffectScope::Single,
            state,
        }
    }

    #[test]
    fn untapping_the_source_itself_is_beneficial_but_tapping_or_untapping_others_is_not() {
        let gs = GameState::new_two_player(7);
        let ai = PlayerId(0);
        assert!(effect_is_beneficial(
            &gs,
            ai,
            &untap(TargetFilter::SelfRef, TapStateChange::Untap)
        ));
        assert!(!effect_is_beneficial(
            &gs,
            ai,
            &untap(TargetFilter::SelfRef, TapStateChange::Tap)
        ));
        assert!(!effect_is_beneficial(
            &gs,
            ai,
            &untap(TargetFilter::Any, TapStateChange::Untap)
        ));
    }

    #[test]
    fn a_draw_from_an_almost_empty_library_is_left_to_the_search() {
        let gs = GameState::new_two_player(7);
        let draw = Effect::Draw {
            count: engine::types::ability::QuantityExpr::Fixed { value: 1 },
            target: TargetFilter::Controller,
        };
        assert!(
            gs.players[0].library.len() <= 3,
            "fixture: a fresh state has no library"
        );
        assert!(!effect_is_beneficial(&gs, PlayerId(0), &draw));
    }
}
