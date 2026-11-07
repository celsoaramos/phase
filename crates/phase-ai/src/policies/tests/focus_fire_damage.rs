//! Focus fire — several "deals N damage to any target" triggers that fire
//! together must pile their damage on ONE creature they can kill between them,
//! not spread it over bodies that all survive.
//!
//! Field report (MagicFinder solo table, engine v0.103.0): two Footlight Fiends
//! ("When this creature dies, it deals 1 damage to any target") died blocking.
//! The opponent had a 3-toughness creature and a 4/4, each already marked with
//! 1 combat damage. The AI aimed one ping at each and both survived; two pings
//! on the 3-toughness creature kill it (CR 120.6 + CR 704.5g).
//!
//! Two decisions are pinned, in the order the engine asks them (CR 603.3b):
//!
//! 1. The FIRST trigger picks its target while the second one waits in
//!    `deferred_triggers` → setting up the kill is rewarded (`SETUP_BONUS`).
//! 2. The SECOND trigger picks its target with the first one already on the
//!    stack → finishing the kill is lethal (`LETHAL_BONUS`) and is no longer
//!    penalized as "redundant removal" by `stack_awareness`.

use engine::ai_support::{ActionMetadata, AiDecisionContext, CandidateAction, TacticalClass};
use engine::game::triggers::{PendingTrigger, PendingTriggerContext};
use engine::game::zones::create_object;
use engine::types::ability::{
    Effect, EffectKind, QuantityExpr, ResolvedAbility, TargetFilter, TargetRef,
};
use engine::types::actions::GameAction;
use engine::types::card_type::{CardType, CoreType};
use engine::types::game_state::{
    GameState, StackEntry, StackEntryKind, TargetEffectDetail, TargetSelectionProgress,
    TargetSelectionSlot, WaitingFor,
};
use engine::types::identifiers::{CardId, ObjectId};
use engine::types::player::PlayerId;
use engine::types::zones::Zone;

use crate::config::AiConfig;
use crate::context::AiContext;
use crate::policies::context::{PolicyContext, SearchDepth};
use crate::policies::removal_lethality::{
    completes_stack_kill, lethality_bonus, queued_follow_up_damage, stack_committed_damage,
    LETHAL_BONUS, SETUP_BONUS,
};
use crate::policies::stack_awareness::StackAwarenessPolicy;

const AI: PlayerId = PlayerId(0);
const OPP: PlayerId = PlayerId(1);

/// "Deals 1 damage to any target" — the Footlight Fiend death trigger.
fn ping() -> Effect {
    Effect::DealDamage {
        amount: QuantityExpr::Fixed { value: 1 },
        target: TargetFilter::Any,
        damage_source: None,
        excess: None,
    }
}

fn opponent_creature(
    state: &mut GameState,
    name: &str,
    power: i32,
    toughness: i32,
    marked: u32,
) -> ObjectId {
    let id = create_object(
        state,
        CardId(state.next_object_id),
        OPP,
        name.to_string(),
        Zone::Battlefield,
    );
    let obj = state.objects.get_mut(&id).unwrap();
    obj.card_types = CardType {
        supertypes: Vec::new(),
        core_types: vec![CoreType::Creature],
        subtypes: Vec::new(),
    };
    obj.power = Some(power);
    obj.toughness = Some(toughness);
    obj.base_power = Some(power);
    obj.base_toughness = Some(toughness);
    obj.damage_marked = marked;
    id
}

/// A dead Footlight Fiend: the trigger's source, already in the graveyard.
fn dead_fiend(state: &mut GameState) -> ObjectId {
    create_object(
        state,
        CardId(state.next_object_id),
        AI,
        "Footlight Fiend".to_string(),
        Zone::Graveyard,
    )
}

fn fiend_trigger(source: ObjectId) -> PendingTrigger {
    PendingTrigger {
        source_id: source,
        controller: AI,
        condition: None,
        ability: Box::new(ResolvedAbility::new(ping(), Vec::new(), source, AI)),
        timestamp: 1,
        target_constraints: Vec::new(),
        distribute: None,
        trigger_event: None,
        modal: None,
        mode_abilities: Vec::new(),
        description: None,
        may_trigger_origin: None,
        subject_match_count: None,
        die_result: None,
        provenance: None,
    }
}

/// Put an already-targeted ping from `source` on the stack.
fn push_ping_on_stack(state: &mut GameState, source: ObjectId, target: ObjectId) {
    let id = ObjectId(state.next_object_id);
    state.next_object_id += 1;
    state.stack.push_back(StackEntry {
        id,
        source_id: source,
        controller: AI,
        kind: StackEntryKind::TriggeredAbility {
            source_id: source,
            ability: Box::new(ResolvedAbility::new(
                ping(),
                vec![TargetRef::Object(target)],
                source,
                AI,
            )),
            condition: None,
            trigger_event: None,
            description: None,
            source_name: "Footlight Fiend".to_string(),
            subject_match_count: None,
            die_result: None,
            provenance: None,
        },
    });
}

/// The board of the field report.
struct Board {
    state: GameState,
    /// 3 toughness, 1 combat damage marked: two pings kill it.
    cultivator: ObjectId,
    /// 4/4 (power doubled to 16 this turn), 1 marked: two pings do not.
    harmonizer: ObjectId,
    first_fiend: ObjectId,
    second_fiend: ObjectId,
}

fn board() -> Board {
    let mut state = GameState::new_two_player(42);
    let cultivator = opponent_creature(&mut state, "Carnivorous Cultivator", 8, 3, 1);
    let harmonizer = opponent_creature(&mut state, "Mightform Harmonizer", 16, 4, 1);
    let first_fiend = dead_fiend(&mut state);
    let second_fiend = dead_fiend(&mut state);
    Board {
        state,
        cultivator,
        harmonizer,
        first_fiend,
        second_fiend,
    }
}

/// Decision 1: the first Fiend's trigger is choosing; the second is queued.
fn first_choice(board: &mut Board) {
    board.state.pending_trigger = Some(Box::new(fiend_trigger(board.first_fiend)));
    board.state.deferred_triggers = vec![PendingTriggerContext::single(fiend_trigger(
        board.second_fiend,
    ))];
}

/// Decision 2: the first Fiend's ping is on the stack aimed at `first_target`;
/// the second Fiend's trigger is choosing.
fn second_choice(board: &mut Board, first_target: ObjectId) {
    push_ping_on_stack(&mut board.state, board.first_fiend, first_target);
    board.state.pending_trigger = Some(Box::new(fiend_trigger(board.second_fiend)));
    board.state.deferred_triggers.clear();
}

/// Run `probe` with the AI choosing `target` for the trigger currently in
/// `state.pending_trigger`.
fn choosing<R>(
    state: &GameState,
    target: ObjectId,
    probe: impl FnOnce(&PolicyContext<'_>) -> R,
) -> R {
    let source = state.pending_trigger.as_ref().map(|t| t.source_id);
    let decision = AiDecisionContext {
        waiting_for: WaitingFor::TriggerTargetSelection {
            player: AI,
            trigger_controller: Some(AI),
            trigger_event: None,
            trigger_events: Vec::new(),
            target_slots: vec![TargetSelectionSlot {
                legal_targets: Vec::new(),
                optional: false,
                chooser: None,
                effect_kind: EffectKind::DealDamage,
                effect_detail: TargetEffectDetail::None,
            }],
            mode_labels: Vec::new(),
            target_constraints: Vec::new(),
            selection: TargetSelectionProgress::default(),
            source_id: source,
            description: None,
        },
        candidates: Vec::new(),
    };
    let candidate = CandidateAction {
        action: GameAction::ChooseTarget {
            target: Some(TargetRef::Object(target)),
        },
        metadata: ActionMetadata::for_actor(Some(AI), TacticalClass::Target),
    };
    let config = AiConfig::default();
    let context = AiContext::empty(&config.weights);
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
    probe(&ctx)
}

fn bonus(state: &GameState, target: ObjectId) -> f64 {
    choosing(state, target, |ctx| {
        lethality_bonus(ctx, target, ctx.state.objects.get(&target).unwrap())
    })
}

#[test]
fn first_trigger_sets_up_the_kill_its_queued_twin_can_finish() {
    let mut b = board();
    first_choice(&mut b);

    let cultivator = bonus(&b.state, b.cultivator);
    let harmonizer = bonus(&b.state, b.harmonizer);
    assert!(
        (cultivator - SETUP_BONUS).abs() < 1e-9,
        "1 + 1 queued on a 3-toughness creature with 1 marked is a kill to set up, got {cultivator}"
    );
    assert!(
        harmonizer < 0.0,
        "1 + 1 queued on a 4/4 with 1 marked kills nothing, got {harmonizer}"
    );
}

#[test]
fn second_trigger_finishes_the_creature_the_first_one_hit() {
    let mut b = board();
    let cultivator_id = b.cultivator;
    second_choice(&mut b, cultivator_id);

    let cultivator = bonus(&b.state, b.cultivator);
    let harmonizer = bonus(&b.state, b.harmonizer);
    assert!(
        (cultivator - LETHAL_BONUS).abs() < 1e-9,
        "the ping on the stack + this ping kill the Cultivator, got {cultivator}"
    );
    assert!(
        harmonizer < 0.0,
        "a lone ping on the 4/4 is a waste, got {harmonizer}"
    );

    assert!(choosing(
        &b.state,
        b.cultivator,
        |ctx| completes_stack_kill(ctx, b.cultivator)
    ));
    assert!(!choosing(&b.state, b.harmonizer, |ctx| {
        completes_stack_kill(ctx, b.harmonizer)
    }));
}

#[test]
fn finishing_shot_is_not_penalized_as_redundant_removal() {
    let mut b = board();
    let cultivator_id = b.cultivator;
    second_choice(&mut b, cultivator_id);

    let policy = StackAwarenessPolicy;
    let on_cultivator = choosing(&b.state, b.cultivator, |ctx| policy.score(ctx));
    let on_harmonizer = choosing(&b.state, b.harmonizer, |ctx| policy.score(ctx));
    assert!(
        on_cultivator >= on_harmonizer,
        "the finishing shot must not rank below an untouched survivor: \
         {on_cultivator} vs {on_harmonizer}"
    );
    assert_eq!(on_cultivator, 0.0);
}

#[test]
fn a_ping_already_on_a_body_it_cannot_kill_still_reads_as_a_waste() {
    // First ping went to the 4/4; the second one cannot finish it either
    // (1 marked + 1 + 1 = 3 < 4), so committed damage must not invent a kill.
    let mut b = board();
    let harmonizer_id = b.harmonizer;
    second_choice(&mut b, harmonizer_id);

    assert!(bonus(&b.state, b.harmonizer) < 0.0);
    assert!(!choosing(&b.state, b.harmonizer, |ctx| {
        completes_stack_kill(ctx, b.harmonizer)
    }));
}

#[test]
fn without_a_queued_twin_a_lone_ping_is_still_a_waste() {
    // Control: the same first choice with nothing queued behind it keeps the
    // pre-existing waste penalty — the setup reward needs a real follow-up.
    let mut b = board();
    b.state.pending_trigger = Some(Box::new(fiend_trigger(b.first_fiend)));

    assert!(bonus(&b.state, b.cultivator) < 0.0);
    let follow_up = choosing(&b.state, b.cultivator, |ctx| {
        queued_follow_up_damage(
            ctx,
            b.cultivator,
            ctx.state.objects.get(&b.cultivator).unwrap(),
        )
    });
    assert!(follow_up.is_empty());
}

#[test]
fn opponent_queued_damage_is_not_the_ai_follow_up() {
    // A queued trigger the OPPONENT controls picks its own target; the AI
    // cannot count on it to finish anything.
    let mut b = board();
    first_choice(&mut b);
    for queued in &mut b.state.deferred_triggers {
        queued.pending.controller = OPP;
        queued.pending.ability.controller = OPP;
    }
    assert!(bonus(&b.state, b.cultivator) < 0.0);
}

#[test]
fn committed_damage_counts_only_what_targets_the_creature() {
    let mut b = board();
    let cultivator_id = b.cultivator;
    second_choice(&mut b, cultivator_id);

    let (on_cultivator, on_harmonizer) = choosing(&b.state, b.cultivator, |ctx| {
        (
            stack_committed_damage(
                ctx,
                b.cultivator,
                ctx.state.objects.get(&b.cultivator).unwrap(),
            ),
            stack_committed_damage(
                ctx,
                b.harmonizer,
                ctx.state.objects.get(&b.harmonizer).unwrap(),
            ),
        )
    });
    assert_eq!(on_cultivator.marked, 1);
    assert!(on_harmonizer.is_empty());
}
