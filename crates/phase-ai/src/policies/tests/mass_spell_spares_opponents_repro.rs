//! Bot flag of 12/11/2026: the AI cast Aetherize in its OWN end step, with no
//! creature attacking — the spell returned nothing. A mass battlefield move
//! (destroy / return / exile all …) whose population holds no opposing permanent
//! is a dead card or a self-sweep; the cast is the window to decline.
//!
//! ```text
//! Aetherize   {3}{U}  Instant
//!   Return all attacking creatures to their owner's hand.
//! Evacuation  {3}{U}{U}  Instant
//!   Return all creatures to their owners' hands.
//! ```

use std::sync::Arc;

use engine::ai_support::{ActionMetadata, AiDecisionContext, CandidateAction, TacticalClass};
use engine::game::combat::{AttackTarget, AttackerInfo, CombatState};
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
const REJECT: &str = "anti_self_harm_mass_spell_spares_opponents";

fn creature(state: &mut GameState, card: u64, owner: PlayerId) -> ObjectId {
    let id = create_object(
        state,
        CardId(card),
        owner,
        format!("Bear {card}"),
        Zone::Battlefield,
    );
    let obj = state.objects.get_mut(&id).unwrap();
    obj.card_types.core_types.push(CoreType::Creature);
    obj.power = Some(2);
    obj.toughness = Some(2);
    id
}

fn instant(state: &mut GameState, name: &str, text: &str) -> (ObjectId, CardId) {
    let card = CardId(9601);
    let id = create_object(state, card, AI, name.to_string(), Zone::Hand);
    let parsed = parse_oracle_text(text, name, &[], &["Instant".to_string()], &[]);
    let obj = state.objects.get_mut(&id).unwrap();
    obj.card_types.core_types.push(CoreType::Instant);
    *Arc::make_mut(&mut obj.abilities) = parsed.abilities;
    (id, card)
}

fn aetherize(state: &mut GameState) -> (ObjectId, CardId) {
    instant(
        state,
        "Aetherize",
        "Return all attacking creatures to their owner's hand.",
    )
}

fn attacking(state: &mut GameState, attacker: ObjectId, defender: PlayerId) {
    let mut combat = CombatState::default();
    combat.attackers.push(AttackerInfo {
        object_id: attacker,
        defending_player: defender,
        attack_target: AttackTarget::Player(defender),
        blocked: false,
        band_id: None,
    });
    state.combat = Some(combat);
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

fn is_mass_reject(verdict: &PolicyVerdict) -> bool {
    matches!(verdict, PolicyVerdict::Reject { reason } if reason.kind == REJECT)
}

#[test]
fn aetherize_with_no_attacking_creature_is_rejected() {
    let mut state = GameState::new_two_player(7);
    creature(&mut state, 9001, OPP);
    creature(&mut state, 9002, AI);
    let (id, card) = aetherize(&mut state);
    let v = verdict(&state, id, card);
    assert!(
        is_mass_reject(&v),
        "expected the mass-spell reject, got {v:?}"
    );
}

#[test]
fn aetherize_returning_only_the_ais_own_attacker_is_rejected() {
    let mut state = GameState::new_two_player(7);
    creature(&mut state, 9001, OPP);
    let own = creature(&mut state, 9002, AI);
    attacking(&mut state, own, OPP);
    let (id, card) = aetherize(&mut state);
    let v = verdict(&state, id, card);
    assert!(
        is_mass_reject(&v),
        "expected the mass-spell reject, got {v:?}"
    );
}

#[test]
fn aetherize_against_an_opposing_attacker_stays_castable() {
    let mut state = GameState::new_two_player(7);
    let theirs = creature(&mut state, 9001, OPP);
    attacking(&mut state, theirs, AI);
    let (id, card) = aetherize(&mut state);
    assert!(!is_mass_reject(&verdict(&state, id, card)));
}

#[test]
fn evacuation_with_an_opposing_creature_stays_castable() {
    let mut state = GameState::new_two_player(7);
    creature(&mut state, 9001, OPP);
    creature(&mut state, 9002, AI);
    let (id, card) = instant(
        &mut state,
        "Evacuation",
        "Return all creatures to their owners' hands.",
    );
    assert!(!is_mass_reject(&verdict(&state, id, card)));
}

#[test]
fn evacuation_with_only_own_creatures_is_rejected() {
    let mut state = GameState::new_two_player(7);
    creature(&mut state, 9002, AI);
    let (id, card) = instant(
        &mut state,
        "Evacuation",
        "Return all creatures to their owners' hands.",
    );
    assert!(is_mass_reject(&verdict(&state, id, card)));
}
