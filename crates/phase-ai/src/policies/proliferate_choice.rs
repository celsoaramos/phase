//! Proliferate selection policy.
//!
//! Report (MagicFinder 05/11, Metastatic Evangel): with its opponent's energy
//! as the only thing carrying a counter, the AI proliferated it — the board
//! evaluation gives energy no value, so "all" tied with "none" and won by
//! candidate order. Each chosen permanent/player is scored by whether one more
//! of each counter on it helps the proliferating player (the engine's
//! `proliferate_target_helps`): each helpful choice adds, unclear ones are
//! neutral, and a selection with anything that helps an opponent is rejected.
//!
//! CR 701.34a: proliferate chooses any number of permanents and/or players
//! that have a counter.

use engine::ai_support::proliferate_target_helps;
use engine::types::actions::GameAction;
use engine::types::game_state::{GameState, WaitingFor};
use engine::types::player::PlayerId;

use super::context::PolicyContext;
use super::registry::{DecisionKind, PolicyId, PolicyReason, PolicyVerdict, TacticalPolicy};
use crate::features::DeckFeatures;

/// One chosen permanent/player whose counters help / hurt the chooser.
const HELPFUL: f64 = 3.0;

pub struct ProliferateChoicePolicy;

impl TacticalPolicy for ProliferateChoicePolicy {
    fn id(&self) -> PolicyId {
        PolicyId::ProliferateChoice
    }

    fn decision_kinds(&self) -> &'static [DecisionKind] {
        // `decision_kind::classify` routes `ProliferateChoice` to `ActivateAbility`.
        &[DecisionKind::ActivateAbility]
    }

    fn activation(
        &self,
        _features: &DeckFeatures,
        state: &GameState,
        _player: PlayerId,
    ) -> Option<f32> {
        // activation-constant: prompt-shape gate, no deck scaling.
        matches!(state.waiting_for, WaitingFor::ProliferateChoice { .. }).then_some(1.0)
    }

    fn verdict(&self, ctx: &PolicyContext<'_>) -> PolicyVerdict {
        let na = || PolicyVerdict::neutral(PolicyReason::new("proliferate_choice_na"));

        let WaitingFor::ProliferateChoice { player, .. } = &ctx.decision.waiting_for else {
            return na();
        };
        if *player != ctx.ai_player {
            return na();
        }
        let GameAction::SelectTargets { targets } = &ctx.candidate.action else {
            return na();
        };

        let (mut helpful, mut harmful) = (0_i64, 0_i64);
        for target in targets {
            match proliferate_target_helps(ctx.state, ctx.ai_player, target) {
                Some(true) => helpful += 1,
                Some(false) => harmful += 1,
                None => {}
            }
        }

        // A choice that helps an opponent is never needed: "none" and the
        // helpful subset (or its singleton) are always offered beside it, and
        // Easy's sampling temperature would otherwise still pick it sometimes.
        if harmful > 0 {
            return PolicyVerdict::reject(
                PolicyReason::new("proliferate_choice_helps_opponent")
                    .with_fact("harmful", harmful),
            );
        }

        PolicyVerdict::score(
            helpful as f64 * HELPFUL,
            PolicyReason::new("proliferate_choice")
                .with_fact("helpful", helpful)
                .with_fact("harmful", harmful),
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
    use engine::types::ability::TargetRef;
    use engine::types::counter::CounterType;
    use engine::types::identifiers::{CardId, ObjectId};
    use engine::types::zones::Zone;

    const AI: PlayerId = PlayerId(0);
    const OPP: PlayerId = PlayerId(1);

    fn score(state: &GameState, eligible: &[TargetRef], targets: Vec<TargetRef>) -> f64 {
        let decision = AiDecisionContext {
            waiting_for: WaitingFor::ProliferateChoice {
                player: AI,
                eligible: eligible.to_vec(),
            },
            candidates: Vec::new(),
        };
        let config = AiConfig::default();
        let context = AiContext::empty(&config.weights);
        let candidate = CandidateAction {
            action: GameAction::SelectTargets { targets },
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
        match ProliferateChoicePolicy.verdict(&ctx) {
            PolicyVerdict::Score { delta, .. } => delta,
            PolicyVerdict::Reject { .. } => f64::NEG_INFINITY,
        }
    }

    fn creature(state: &mut GameState, owner: PlayerId, counter: CounterType) -> ObjectId {
        let id = create_object(state, CardId(7), owner, "Bear".into(), Zone::Battlefield);
        let obj = state.objects.get_mut(&id).unwrap();
        obj.controller = owner;
        obj.counters.insert(counter, 2);
        id
    }

    #[test]
    fn never_gives_the_opponent_energy() {
        // The report: the opponent's energy was the only eligible choice.
        let mut state = GameState::new_two_player(1);
        state.players[1].energy = 1;
        let eligible = vec![TargetRef::Player(OPP)];
        let all = score(&state, &eligible, eligible.clone());
        let none = score(&state, &eligible, Vec::new());
        assert!(none > all, "none {none} should beat giving energy {all}");
    }

    #[test]
    fn grows_its_own_creature_and_not_the_opponents() {
        let mut state = GameState::new_two_player(1);
        let mine = creature(&mut state, AI, CounterType::Plus1Plus1);
        let theirs = creature(&mut state, OPP, CounterType::Plus1Plus1);
        let eligible = vec![TargetRef::Object(mine), TargetRef::Object(theirs)];
        let only_mine = score(&state, &eligible, vec![TargetRef::Object(mine)]);
        let all = score(&state, &eligible, eligible.clone());
        let none = score(&state, &eligible, Vec::new());
        assert!(only_mine > all && only_mine > none);
    }

    #[test]
    fn adds_poison_to_the_opponent_and_minus_counters_to_their_creature() {
        let mut state = GameState::new_two_player(1);
        state.players[1].poison_counters = 3;
        let theirs = creature(&mut state, OPP, CounterType::Minus1Minus1);
        let eligible = vec![TargetRef::Player(OPP), TargetRef::Object(theirs)];
        let all = score(&state, &eligible, eligible.clone());
        assert!(all > score(&state, &eligible, vec![TargetRef::Player(OPP)]));
        assert!(score(&state, &eligible, vec![TargetRef::Player(OPP)]) > 0.0);
    }

    #[test]
    fn own_energy_is_good() {
        let mut state = GameState::new_two_player(1);
        state.players[0].energy = 2;
        let eligible = vec![TargetRef::Player(AI)];
        assert!(score(&state, &eligible, eligible.clone()) > 0.0);
    }

    #[test]
    fn engine_offers_the_helpful_subset() {
        // Two of mine and one of theirs: the best selection (both of mine) is
        // neither "all", "none" nor a singleton, so the engine must offer it.
        let mut state = GameState::new_two_player(1);
        let a = creature(&mut state, AI, CounterType::Plus1Plus1);
        let b = creature(&mut state, AI, CounterType::Plus1Plus1);
        let theirs = creature(&mut state, OPP, CounterType::Plus1Plus1);
        let eligible = vec![
            TargetRef::Object(a),
            TargetRef::Object(theirs),
            TargetRef::Object(b),
        ];
        state.waiting_for = WaitingFor::ProliferateChoice {
            player: AI,
            eligible: eligible.clone(),
        };
        let offered = engine::ai_support::candidate_actions(&state);
        let want = vec![TargetRef::Object(a), TargetRef::Object(b)];
        assert!(
            offered.iter().any(|c| matches!(
                &c.action,
                GameAction::SelectTargets { targets } if *targets == want
            )),
            "helpful subset not offered"
        );
    }

    #[test]
    fn registry_registers_proliferate_choice() {
        assert!(PolicyRegistry::default().has_policy(PolicyId::ProliferateChoice));
    }
}
