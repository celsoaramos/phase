//! "Tap or untap target permanent" branch policy.
//!
//! Report (MagicFinder 27/10, Sewer-veillance Cam): the Cam's trigger picked the
//! right target (the opponent's untapped Master of Etherium) but the resolution
//! prompt — `WaitingFor::ChooseOneOfBranch` with a "tap" and an "untap" branch —
//! has no policy scoring it, so the tie fell to candidate order / sampling and
//! the AI could pick "untap" on an already untapped creature: the opponent's
//! Master then attacked. Nothing was wrong in the engine.
//!
//! Each branch is scored by what it does to the parent's targets:
//! tapping an untapped opposing permanent (or untapping the AI's own tapped one)
//! is the useful half; the opposite direction is harmful; a no-op change is a
//! mild negative so a useful branch always outranks it. Branches that are not a
//! single-scope `SetTapState` on the parent target are left to the old tie-break.
//!
//! CR 701.26a-b: tap / untap. CR 608.2d: the choice is made on resolution.

use engine::types::ability::{Effect, EffectScope, TapStateChange, TargetFilter, TargetRef};
use engine::types::actions::GameAction;
use engine::types::game_state::{GameState, WaitingFor};
use engine::types::player::PlayerId;

use super::context::PolicyContext;
use super::registry::{DecisionKind, PolicyId, PolicyReason, PolicyVerdict, TacticalPolicy};
use crate::features::DeckFeatures;

/// Useful change (tap their untapped, untap our tapped) / harmful change.
const USEFUL: f64 = 4.0;
const HARMFUL: f64 = -4.0;
/// No-op change: never preferred over a useful branch.
const NO_OP: f64 = -2.0;

pub struct TapStateBranchPolicy;

impl TacticalPolicy for TapStateBranchPolicy {
    fn id(&self) -> PolicyId {
        PolicyId::TapStateBranch
    }

    fn decision_kinds(&self) -> &'static [DecisionKind] {
        // `decision_kind::classify` routes `ChooseOneOfBranch` to `ActivateAbility`.
        &[DecisionKind::ActivateAbility]
    }

    fn activation(
        &self,
        _features: &DeckFeatures,
        state: &GameState,
        _player: PlayerId,
    ) -> Option<f32> {
        // activation-constant: prompt-shape gate, no deck scaling.
        matches!(state.waiting_for, WaitingFor::ChooseOneOfBranch { .. }).then_some(1.0)
    }

    fn verdict(&self, ctx: &PolicyContext<'_>) -> PolicyVerdict {
        let na = || PolicyVerdict::neutral(PolicyReason::new("tap_state_branch_na"));

        let WaitingFor::ChooseOneOfBranch {
            player,
            branches,
            parent_occurrences,
            ..
        } = &ctx.decision.waiting_for
        else {
            return na();
        };
        if *player != ctx.ai_player {
            return na();
        }
        let GameAction::ChooseBranch { index } = &ctx.candidate.action else {
            return na();
        };
        let Some(branch) = branches.get(*index) else {
            return na();
        };
        let Effect::SetTapState {
            target: TargetFilter::ParentTarget,
            scope: EffectScope::Single,
            state,
        } = branch.effect.as_ref()
        else {
            return na();
        };

        let mut total = 0.0;
        let mut seen = 0_i64;
        for target in &parent_occurrences.targets {
            let TargetRef::Object(id) = target else {
                continue;
            };
            let Some(object) = ctx.state.objects.get(id) else {
                continue;
            };
            seen += 1;
            let mine = object.controller == ctx.ai_player;
            total += match (state, object.tapped, mine) {
                // Tap: good against an untapped opposing permanent.
                (TapStateChange::Tap, false, false) => USEFUL,
                (TapStateChange::Tap, false, true) => HARMFUL,
                (TapStateChange::Tap, true, _) => NO_OP,
                // Untap: good for our own tapped permanent.
                (TapStateChange::Untap, true, true) => USEFUL,
                (TapStateChange::Untap, true, false) => HARMFUL,
                (TapStateChange::Untap, false, _) => NO_OP,
            };
        }
        if seen == 0 {
            return na();
        }

        PolicyVerdict::score(
            total,
            PolicyReason::new("tap_state_branch").with_fact("targets", seen),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AiConfig;
    use crate::context::AiContext;
    use crate::policies::context::SearchDepth;
    use crate::policies::registry::PolicyRegistry;
    use engine::ai_support::{ActionMetadata, AiDecisionContext, CandidateAction, TacticalClass};
    use engine::game::zones::create_object;
    use engine::types::ability::{AbilityDefinition, AbilityKind};
    use engine::types::game_state::ParentTargetOccurrences;
    use engine::types::identifiers::{CardId, ObjectId};
    use engine::types::zones::Zone;

    const AI: PlayerId = PlayerId(0);
    const OPP: PlayerId = PlayerId(1);

    fn branch(state: TapStateChange) -> AbilityDefinition {
        AbilityDefinition::new(
            AbilityKind::Spell,
            Effect::SetTapState {
                target: TargetFilter::ParentTarget,
                scope: EffectScope::Single,
                state,
            },
        )
    }

    fn decision(target: ObjectId) -> AiDecisionContext {
        AiDecisionContext {
            waiting_for: WaitingFor::ChooseOneOfBranch {
                player: AI,
                controller: AI,
                source_id: target,
                branches: vec![branch(TapStateChange::Tap), branch(TapStateChange::Untap)],
                branch_descriptions: vec!["tap".into(), "untap".into()],
                parent_occurrences: ParentTargetOccurrences {
                    targets: vec![TargetRef::Object(target)],
                    pins: Vec::new(),
                },
                context: Default::default(),
                continuation: None,
                replacement_applied: Default::default(),
                remaining_players: Vec::new(),
                trigger_context: None,
            },
            candidates: Vec::new(),
        }
    }

    fn score(state: &GameState, target: ObjectId, index: usize) -> f64 {
        let decision = decision(target);
        let config = AiConfig::default();
        let context = AiContext::empty(&config.weights);
        let candidate = CandidateAction {
            action: GameAction::ChooseBranch { index },
            metadata: ActionMetadata::for_actor(Some(AI), TacticalClass::Selection),
        };
        let ctx = PolicyContext {
            state,
            decision: &decision,
            candidate: &candidate,
            ai_player: AI,
            config: &config,
            context: &context,
            cast_facts: None,
            search_depth: SearchDepth::Root,
        };
        match TapStateBranchPolicy.verdict(&ctx) {
            PolicyVerdict::Score { delta, .. } => delta,
            PolicyVerdict::Reject { .. } => panic!("unexpected Reject"),
        }
    }

    fn put(state: &mut GameState, owner: PlayerId, tapped: bool) -> ObjectId {
        let id = create_object(state, CardId(7), owner, "Bear".into(), Zone::Battlefield);
        let obj = state.objects.get_mut(&id).unwrap();
        obj.controller = owner;
        obj.tapped = tapped;
        id
    }

    #[test]
    fn taps_an_untapped_opposing_creature() {
        let mut state = GameState::new_two_player(1);
        let id = put(&mut state, OPP, false);
        assert!(score(&state, id, 0) > score(&state, id, 1));
        assert!(score(&state, id, 0) > 0.0);
    }

    #[test]
    fn untaps_our_own_tapped_creature() {
        let mut state = GameState::new_two_player(1);
        let id = put(&mut state, AI, true);
        assert!(score(&state, id, 1) > score(&state, id, 0));
        assert!(score(&state, id, 1) > 0.0);
    }

    #[test]
    fn never_untaps_a_tapped_opposing_creature_when_tap_is_a_no_op() {
        let mut state = GameState::new_two_player(1);
        let id = put(&mut state, OPP, true);
        assert!(score(&state, id, 0) > score(&state, id, 1));
    }

    #[test]
    fn registry_registers_tap_state_branch() {
        assert!(PolicyRegistry::default().has_policy(PolicyId::TapStateBranch));
    }
}
