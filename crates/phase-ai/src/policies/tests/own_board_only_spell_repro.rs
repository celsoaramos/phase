//! Bot flag of 09/11/2026 ("destruiu o próprio artefato"): the AI cast Abrade
//! with no creature in play and only its OWN artifacts on the battlefield, so the
//! only legal target of the "Destroy target artifact" mode was its own Fellwar
//! Stone. The cast is the last window to decline (CR 601.2c targets, CR 601.2h
//! costs), so the own-board veto that already covered activations now covers
//! instant and sorcery casts too.
//!
//! ```text
//! Abrade  {1}{R}  Instant
//!   Choose one —
//!   • Abrade deals 3 damage to target creature.
//!   • Destroy target artifact.
//! ```

use std::sync::Arc;

use engine::ai_support::{ActionMetadata, AiDecisionContext, CandidateAction, TacticalClass};
use engine::game::zones::create_object;
use engine::parser::oracle::parse_oracle_text;
use engine::types::actions::GameAction;
use engine::types::card_type::CoreType;
use engine::types::game_state::{CastPaymentMode, GameState, WaitingFor};
use engine::types::identifiers::{CardId, ObjectId};
use engine::types::player::PlayerId;
use engine::types::zones::Zone;

use crate::config::AiConfig;
use crate::context::AiContext;
use crate::policies::anti_self_harm::AntiSelfHarmPolicy;
use crate::policies::context::{PolicyContext, SearchDepth};
use crate::policies::registry::{PolicyVerdict, TacticalPolicy};

const AI: PlayerId = PlayerId(0);
const OPP: PlayerId = PlayerId(1);

fn artifact(state: &mut GameState, card: u64, owner: PlayerId, name: &str) -> ObjectId {
    let id = create_object(state, CardId(card), owner, name.to_string(), Zone::Battlefield);
    state.objects.get_mut(&id).unwrap().card_types.core_types.push(CoreType::Artifact);
    id
}

fn abrade(state: &mut GameState) -> (ObjectId, CardId) {
    let card = CardId(9501);
    let id = create_object(state, card, AI, "Abrade".to_string(), Zone::Hand);
    let parsed = parse_oracle_text(
        "Choose one —\n• Abrade deals 3 damage to target creature.\n• Destroy target artifact.",
        "Abrade",
        &[],
        &["Instant".to_string()],
        &[],
    );
    let obj = state.objects.get_mut(&id).unwrap();
    obj.card_types.core_types.push(CoreType::Instant);
    *Arc::make_mut(&mut obj.abilities) = parsed.abilities;
    (id, card)
}

fn verdict(state: &GameState, object_id: ObjectId, card_id: CardId) -> PolicyVerdict {
    let candidate = CandidateAction {
        action: GameAction::CastSpell {
            object_id,
            card_id,
            targets: Vec::new(),
            payment_mode: CastPaymentMode::Auto,
        },
        metadata: ActionMetadata::for_actor(Some(AI), TacticalClass::Spell),
    };
    let decision = AiDecisionContext {
        waiting_for: WaitingFor::Priority { player: AI },
        candidates: Vec::new(),
    };
    let config = AiConfig::default();
    let mut context = AiContext::empty(&config.weights);
    context.player = AI;
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
    AntiSelfHarmPolicy.verdict(&ctx)
}

#[test]
fn abrade_with_only_own_artifacts_is_rejected() {
    let mut state = GameState::new_two_player(7);
    artifact(&mut state, 9001, AI, "Fellwar Stone");
    artifact(&mut state, 9002, AI, "Shadowspear");
    let (id, card) = abrade(&mut state);
    match verdict(&state, id, card) {
        PolicyVerdict::Reject { reason } => {
            assert_eq!(reason.kind, "anti_self_harm_harmful_spell_own_board_only")
        }
        other => panic!("expected Reject, got {other:?}"),
    }
}

#[test]
fn an_opposing_artifact_makes_abrade_castable_again() {
    let mut state = GameState::new_two_player(7);
    artifact(&mut state, 9001, AI, "Fellwar Stone");
    artifact(&mut state, 9003, OPP, "Opposing Relic");
    let (id, card) = abrade(&mut state);
    assert!(
        matches!(verdict(&state, id, card), PolicyVerdict::Score { .. }),
        "an opponent artifact is a real target — the veto must not fire"
    );
}
