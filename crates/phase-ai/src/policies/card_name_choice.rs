//! Card-name named-choice policy.
//!
//! Report (owner, 2026-10-30): the robot names a card "without criteria".
//! Reproduced in `tests/ai_quality.rs`: Cabal Therapy against an opponent with
//! two Lightning Bolts in the graveyard named the robot's OWN Thoughtseize —
//! no policy scored `ChooseOption` for a `ChoiceType::CardName` prompt, so the
//! first candidate won, and the candidate list filled with the chooser's own
//! deck before reaching the opponent's graveyard (fixed in
//! `engine::ai_support::candidates`).
//!
//! The cards that make the AI name a card almost all aim it at an opponent: a
//! discard (Cabal Therapy, Brain Pry), a lock (Meddling Mage, Pithing Needle,
//! Nevermore), a hoser (Slaughter Games). So a name scores by the evidence
//! that the OPPONENT plays it, read only from what the AI may legally know
//! (CR 400.2): the opponent's battlefield, graveyard and exile, and the cards
//! of theirs revealed at least once. Their hand and library are never read.
//!
//! - A copy revealed while still in the opponent's hand adds
//!   `card_name_known_in_hand` — a sure hit, worth more than any amount of
//!   evidence about the deck.
//! - Each other shown nonland, non-token copy owned by an opponent adds
//!   `card_name_shown_unit`, capped — a playset in the graveyard says the deck
//!   is built around it.
//! - A name the AI only knows from its OWN cards (including the source of the
//!   prompt itself) takes `card_name_own_penalty`: naming it strips nothing
//!   from the opponent and can lock the AI out of its own card.
//!
//! Limitations: lands are not counted as evidence — "choose a nonland card
//! name" is the common restriction, and the engine's `ChoiceType::CardName`
//! does not carry it, so a land name could be illegal. The rare prompt that
//! names a card for the AI's own benefit is not detected and is scored the
//! same way; the "guess the opponent's card" prompts are answered earlier by
//! the search (`random_card_predicate_guess`).

use std::collections::HashSet;

use engine::types::ability::ChoiceType;
use engine::types::actions::GameAction;
use engine::types::card_type::CoreType;
use engine::types::game_state::{GameState, WaitingFor};
use engine::types::identifiers::ObjectId;
use engine::types::player::PlayerId;
use engine::types::zones::Zone;

use super::context::PolicyContext;
use super::registry::{DecisionKind, PolicyId, PolicyReason, PolicyVerdict, TacticalPolicy};
use crate::features::DeckFeatures;

/// Shown copies past the fourth add nothing: four is a full playset.
const SHOWN_CAP: usize = 4;

/// Objects whose name the AI may legally know and that belong to an opponent:
/// face-up permanents, graveyard and face-up exile cards, and cards revealed
/// at least once (CR 400.2 + CR 701.20b).
fn opponent_known_objects(state: &GameState, ai: PlayerId) -> Vec<ObjectId> {
    let mut ids: HashSet<ObjectId> = state
        .public_revealed_cards
        .iter()
        .chain(state.revealed_cards.iter())
        .copied()
        .collect();
    ids.extend(state.battlefield.iter().copied());
    ids.extend(state.exile.iter().copied());
    for opponent in state.players.iter().filter(|p| p.id != ai) {
        ids.extend(state.graveyard_of(opponent.id).iter().copied());
    }
    ids.into_iter()
        .filter(|id| {
            state.objects.get(id).is_some_and(|obj| {
                obj.owner != ai
                    && !obj.is_token
                    && (!obj.face_down
                        || state.public_revealed_cards.contains(id)
                        || state.revealed_cards.contains(id))
            })
        })
        .collect()
}

fn is_land(state: &GameState, id: &ObjectId) -> bool {
    state
        .objects
        .get(id)
        .is_some_and(|obj| obj.card_types.core_types.contains(&CoreType::Land))
}

/// Whether the AI owns any object with this name (it knows its own cards).
/// Identity lookup of the prompt's candidate string, not classification by
/// name: the candidates are copied verbatim from object names.
fn ai_owns_name(state: &GameState, ai: PlayerId, name: &str) -> bool {
    state
        .objects
        .values()
        .any(|obj| obj.owner == ai && name == obj.name)
}

pub struct CardNameChoicePolicy;

// `ChoiceType::CardName { .. }` matches the unit variant of the upstream tag and
// the field-carrying variant of the fork's Garth One-Eye branch alike.
#[allow(clippy::unneeded_struct_pattern)]
impl TacticalPolicy for CardNameChoicePolicy {
    fn id(&self) -> PolicyId {
        PolicyId::CardNameChoice
    }

    fn decision_kinds(&self) -> &'static [DecisionKind] {
        // `decision_kind::classify` routes every `NamedChoice` prompt into the
        // `ActivateAbility` bucket.
        &[DecisionKind::ActivateAbility]
    }

    fn activation(
        &self,
        _features: &DeckFeatures,
        state: &GameState,
        _player: PlayerId,
    ) -> Option<f32> {
        if matches!(
            state.waiting_for,
            WaitingFor::NamedChoice {
                choice_type: ChoiceType::CardName { .. },
                ..
            }
        ) {
            // activation-constant: card-name prompt gate, no deck scaling.
            Some(1.0)
        } else {
            None
        }
    }

    fn verdict(&self, ctx: &PolicyContext<'_>) -> PolicyVerdict {
        let na = || PolicyVerdict::neutral(PolicyReason::new("card_name_choice_na"));

        let WaitingFor::NamedChoice {
            player,
            choice_type: ChoiceType::CardName { .. },
            ..
        } = &ctx.decision.waiting_for
        else {
            return na();
        };
        if *player != ctx.ai_player {
            return na();
        }
        let GameAction::ChooseOption { choice } = &ctx.candidate.action else {
            return na();
        };

        let named: Vec<_> = opponent_known_objects(ctx.state, ctx.ai_player)
            .into_iter()
            .filter(|id| !is_land(ctx.state, id))
            .filter_map(|id| ctx.state.objects.get(&id))
            .filter(|obj| *choice == obj.name)
            .collect();
        // CR 701.20b: a card revealed while still in the opponent's hand is a
        // sure hit for a named discard, not just evidence about the deck.
        let in_hand = named.iter().filter(|obj| obj.zone == Zone::Hand).count();
        let shown = named.len() - in_hand;

        let penalties = &ctx.config.policy_penalties;
        if in_hand + shown > 0 {
            return PolicyVerdict::score(
                in_hand.min(SHOWN_CAP) as f64 * penalties.card_name_known_in_hand
                    + shown.min(SHOWN_CAP) as f64 * penalties.card_name_shown_unit,
                PolicyReason::new("card_name_choice_shown")
                    .with_fact("in_hand", in_hand as i64)
                    .with_fact("shown", shown as i64),
            );
        }
        if ai_owns_name(ctx.state, ctx.ai_player, choice) {
            return PolicyVerdict::score(
                -penalties.card_name_own_penalty,
                PolicyReason::new("card_name_choice_own_card"),
            );
        }
        PolicyVerdict::neutral(PolicyReason::new("card_name_choice_no_signal"))
    }
}
