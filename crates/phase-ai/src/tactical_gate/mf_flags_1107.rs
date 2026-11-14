//! Gates born from the 64 "jogada ruim do robô" flags of 24/10–07/11/2026
//! (MagicFinder, `docs/mesa-ia-estrategia-2026-10.md` §11). Every gate here is a
//! PROOF-backed `Reject`: a soft policy penalty is weighted ×0.1 against the board
//! evaluation under search and the final pick used to be sampled, so "−2" never
//! stopped any of these plays. Each gate fails OPEN on anything it cannot read.
//!
//! The flagged plays, by gate:
//! - [`keyword_change_is_redundant`]: Shadowspear "{1}: permanents your opponents
//!   control lose hexproof and indestructible" activated three times into an empty
//!   opposing board; Bonds of Mortality every turn; Iron-Shield Elf discarding its
//!   whole hand to give indestructible to a creature that already had it.
//! - [`loyalty_put_from_hand_has_no_card`]: Garruk, Caller of Beasts −3 ("you may put
//!   a green creature card from your hand onto the battlefield") with no green
//!   creature in hand — loyalty spent, nothing happened.
//! - [`resolution_life_loss_is_lethal`]: Vampiric Tutor at 1 life ("you lose 2
//!   life" is an EFFECT, so the cast is legal and the AI kills itself).
//! - [`flash_creature_in_opponent_beginning_phase`]: Selfless Safewright flashed in
//!   during the opponent's upkeep — their end step dominates (CR 500.1, the end step
//!   exists; CR 302.6 nothing is gained by entering early).
//! - [`mana_grant_outside_window`]: Plaxcaster Frogling "{2}: target creature with a
//!   +1/+1 counter gains shroud until end of turn" in its own upkeep, spending the
//!   mana the commander needed; Fleeting Spirit first strike at declare attackers
//!   against a board with no blocker.
//! - [`aura_keywords_already_present`]: Mystic Veil (hexproof) on a creature that
//!   already has hexproof.
//! - [`land_removal_with_basic_compensation`]: Assassin's Trophy on the opponent's
//!   only basic Swamp on turn 3 — they search for a basic land, net zero.
//! - [`nonlethal_burn_target_with_lethal_alternative`]: Chain Lightning at a 6/6
//!   with a 2/2 and a 2/3 on the same board.
//! - [`beneficial_target_on_opponent_with_own_alternative`]: Optimistic Scavenger's
//!   "+1/+1 counter on target creature" put on the opponent's creature.

use engine::ai_support::current_target_selection_targets;
use engine::game::filter::{matches_target_filter, FilterContext};
use engine::game::players;
use engine::game::quantity::resolve_quantity;
use engine::game::targeting::find_legal_targets;
use engine::game::turn_control::turn_decision_maker;
use engine::types::ability::{
    AbilityCost, ContinuousModification, CostCategory, Duration, Effect, EffectScope,
    StaticDefinition, TargetFilter, TargetRef,
};
use engine::types::actions::GameAction;
use engine::types::card_type::{CoreType, Supertype};
use engine::types::game_state::GameState;
use engine::types::identifiers::ObjectId;
use engine::types::keywords::Keyword;
use engine::types::phase::Phase;
use engine::types::zones::Zone;

use super::{GateDecision, TacticalFacts, TacticalWindow};
use crate::policies::context::PolicyContext;
use crate::policies::effect_classify::{effect_polarity, is_spell_beneficial, EffectPolarity};
use crate::policies::removal_lethality::{outcome_is_lethal, pending_damage_to_object, PendingDamage};

/// `MF_GATE_TRACE=1` names the gate that rejected a candidate (native only).
fn trace(message: impl FnOnce() -> String) {
    if std::env::var_os("MF_GATE_TRACE").is_some() {
        eprintln!("{}", message());
    }
}

/// Pre-cast / pre-activation gates. `Some(Reject)` removes the candidate.
pub(super) fn assess_pre_cast(ctx: &PolicyContext<'_>) -> Option<GateDecision> {
    let gates: [(&str, fn(&PolicyContext<'_>) -> bool); 6] = [
        ("keyword_change_is_redundant", keyword_change_is_redundant),
        ("loyalty_put_from_hand_has_no_card", loyalty_put_from_hand_has_no_card),
        ("resolution_life_loss_is_lethal", resolution_life_loss_is_lethal),
        ("flash_creature_in_opponent_beginning_phase", flash_creature_in_opponent_beginning_phase),
        ("mana_grant_outside_window", mana_grant_outside_window),
        ("aura_cast_hosts_all_redundant", aura_cast_hosts_all_redundant),
    ];
    for (name, gate) in gates {
        if gate(ctx) {
            trace(|| format!("mf_flags_1107 reject {:?} by {name}", ctx.candidate.action));
            return Some(GateDecision::Reject);
        }
    }
    trace(|| format!("mf_flags_1107 allow {:?} effects={:?}", ctx.candidate.action, ctx.effects()));
    None
}

/// Target-step gates. Also consulted by `cast_targets_are_all_futile`, so a spell
/// whose EVERY legal target is rejected here is not cast at all.
pub(super) fn reject_target(ctx: &PolicyContext<'_>, target: &TargetRef) -> bool {
    let TargetRef::Object(object_id) = target else {
        return false;
    };
    aura_keywords_already_present(ctx, *object_id)
        || land_removal_with_basic_compensation(ctx, *object_id)
        || nonlethal_burn_target_with_lethal_alternative(ctx, *object_id)
        || beneficial_target_on_opponent_with_own_alternative(ctx, *object_id)
}

// ── helpers ──────────────────────────────────────────────────────────────────

fn candidate_source_id(ctx: &PolicyContext<'_>) -> Option<ObjectId> {
    match &ctx.candidate.action {
        GameAction::CastSpell { object_id, .. } => Some(*object_id),
        GameAction::ActivateAbility { source_id, .. } => Some(*source_id),
        _ => None,
    }
}

fn activated_cost<'a>(ctx: &'a PolicyContext<'_>) -> Option<&'a AbilityCost> {
    let GameAction::ActivateAbility {
        source_id,
        ability_index,
        ..
    } = &ctx.candidate.action
    else {
        return None;
    };
    ctx.state
        .objects
        .get(source_id)?
        .abilities
        .get(*ability_index)?
        .cost
        .as_ref()
}

fn cost_has_category(cost: Option<&AbilityCost>, wanted: CostCategory) -> bool {
    cost.is_some_and(|cost| cost.categories().contains(&wanted))
}

/// A rider that moves nothing on the board: "Tap it" on the ability's own source
/// (Iron-Shield Elf: "... gains indestructible until end of turn. Tap it."). The
/// parser's bare "it" is `ParentTarget`; in a chain with no target slot that is
/// the source itself.
fn is_self_tap_rider(effect: &Effect) -> bool {
    matches!(
        effect,
        Effect::SetTapState {
            scope: EffectScope::Single,
            target: TargetFilter::SelfRef | TargetFilter::ParentTarget,
            ..
        }
    )
}

/// Every object on the battlefield that `filter` matches from `source_id`'s point
/// of view (the `affected` population of an untargeted continuous effect).
fn affected_population(state: &GameState, source_id: ObjectId, filter: &TargetFilter) -> Vec<ObjectId> {
    if matches!(filter, TargetFilter::SelfRef) {
        return vec![source_id];
    }
    let fctx = FilterContext::from_source(state, source_id);
    state
        .battlefield
        .iter()
        .copied()
        .filter(|id| matches_target_filter(state, *id, filter, &fctx))
        .collect()
}

#[derive(Clone, Copy)]
enum KeywordChange<'a> {
    Add(&'a Keyword),
    Remove(&'a Keyword),
}

/// `Some(changes)` when EVERY modification of every static is a keyword add or
/// remove; `None` (fail open) for anything else (P/T, abilities, types…).
fn keyword_changes(static_abilities: &[StaticDefinition]) -> Option<Vec<KeywordChange<'_>>> {
    let mut out = Vec::new();
    for stat in static_abilities {
        if stat.condition.is_some() {
            return None;
        }
        for modification in &stat.modifications {
            match modification {
                ContinuousModification::AddKeyword { keyword } => out.push(KeywordChange::Add(keyword)),
                ContinuousModification::RemoveKeyword { keyword } => {
                    out.push(KeywordChange::Remove(keyword))
                }
                _ => return None,
            }
        }
    }
    if out.is_empty() {
        return None;
    }
    Some(out)
}

fn change_is_already_true(state: &GameState, object_id: ObjectId, change: KeywordChange<'_>) -> bool {
    let Some(object) = state.objects.get(&object_id) else {
        return true;
    };
    match change {
        KeywordChange::Add(keyword) => object.has_keyword(keyword),
        KeywordChange::Remove(keyword) => !object.has_keyword(keyword),
    }
}

// ── pre-cast gates ───────────────────────────────────────────────────────────

/// CR 611.2a + CR 702: granting a keyword a permanent already has, or stripping
/// one it does not have, changes nothing. An activation or cast whose entire
/// chain is such keyword changes (plus at most a "tap it" rider on its own source)
/// and whose every recipient already satisfies every change is a no-op — and an
/// empty recipient set is a no-op too (Shadowspear into an empty board).
///
/// Sacrifice costs stay out: a sacrifice outlet has a payoff of its own.
/// Loyalty costs stay out: `loyalty_put_from_hand_has_no_card` and the
/// planeswalker policy own that lane.
fn keyword_change_is_redundant(ctx: &PolicyContext<'_>) -> bool {
    let Some(source_id) = candidate_source_id(ctx) else {
        return false;
    };
    let cost = activated_cost(ctx);
    if cost_has_category(cost, CostCategory::SacrificesPermanent)
        || cost_has_category(cost, CostCategory::PaysLoyalty)
    {
        return false;
    }
    let effects: Vec<&Effect> = ctx
        .effects()
        .into_iter()
        .filter(|effect| !is_self_tap_rider(effect))
        .collect();
    trace(|| format!("keyword_change effects after riders: {}", effects.len()));
    if effects.is_empty() {
        return false;
    }
    for effect in effects {
        let Effect::GenericEffect {
            static_abilities,
            target,
            ..
        } = effect
        else {
            return false;
        };
        let Some(changes) = keyword_changes(static_abilities) else {
            return false;
        };
        // Recipients: the chosen-target population (not chosen yet at this step,
        // so every legal target) or the `affected` population of each static.
        let recipients: Vec<ObjectId> = match target {
            Some(filter) => find_legal_targets(ctx.state, filter, ctx.ai_player, source_id)
                .into_iter()
                .filter_map(|target| match target {
                    TargetRef::Object(id) => Some(id),
                    TargetRef::Player(_) => None,
                })
                .collect(),
            None => {
                let mut out = Vec::new();
                for stat in static_abilities {
                    let Some(affected) = stat.affected.as_ref() else {
                        // A static with no recipient set is not something this
                        // gate can read.
                        return false;
                    };
                    for id in affected_population(ctx.state, source_id, affected) {
                        if !out.contains(&id) {
                            out.push(id);
                        }
                    }
                }
                out
            }
        };
        // Targeted grant with no legal object recipient: the engine decides
        // whether the ability is even offered; not this gate's proof.
        if target.is_some() && recipients.is_empty() {
            return false;
        }
        let all_redundant = recipients
            .iter()
            .all(|&id| changes.iter().all(|change| change_is_already_true(ctx.state, id, *change)));
        trace(|| {
            let keywords: Vec<String> = recipients
                .iter()
                .map(|id| format!("{id:?}:{:?}", ctx.state.objects.get(id).map(|o| o.keywords.clone())))
                .collect();
            format!("keyword_change recipients={recipients:?} keywords={keywords:?} redundant={all_redundant}")
        });
        if !all_redundant {
            return false;
        }
    }
    true
}

/// CR 606: a loyalty ability "you may put a [filter] card from your hand onto the
/// battlefield" with no card in hand that matches spends loyalty for nothing.
/// Every effect in the chain must be such a hand→battlefield move (fail open on
/// anything else: a minus that also draws or makes a token still does something).
fn loyalty_put_from_hand_has_no_card(ctx: &PolicyContext<'_>) -> bool {
    let GameAction::ActivateAbility { source_id, .. } = &ctx.candidate.action else {
        return false;
    };
    if !cost_has_category(activated_cost(ctx), CostCategory::PaysLoyalty) {
        return false;
    }
    let effects = ctx.effects();
    if effects.is_empty() {
        return false;
    }
    let hand = &ctx.state.players[ctx.ai_player.0 as usize].hand;
    let fctx = FilterContext::from_source(ctx.state, *source_id);
    for effect in effects {
        let filter = match effect {
            Effect::ChangeZone {
                origin: Some(Zone::Hand),
                destination: Zone::Battlefield,
                target,
                ..
            } => Some(target),
            Effect::ChooseFromZone {
                zone: Zone::Hand,
                filter,
                ..
            } => filter.as_ref(),
            _ => return false,
        };
        let any_match = hand.iter().any(|card| match filter {
            Some(filter) => matches_target_filter(ctx.state, *card, filter, &fctx),
            None => true,
        });
        if any_match {
            return false;
        }
    }
    true
}

/// CR 119.4 lets a player pay life only as a COST; "you lose N life" during
/// resolution is an effect and is legal at any life total (Vampiric Tutor at 1).
/// CR 704.5a then ends the game. Reject a cast/activation whose own fixed life
/// loss reaches the AI's life total, unless the same chain also wins or gains.
fn resolution_life_loss_is_lethal(ctx: &PolicyContext<'_>) -> bool {
    let Some(source_id) = candidate_source_id(ctx) else {
        return false;
    };
    let effects = ctx.effects();
    if effects
        .iter()
        .any(|effect| matches!(effect, Effect::WinTheGame { .. } | Effect::GainLife { .. }))
    {
        return false;
    }
    let mut loss = 0i32;
    for effect in effects {
        if let Effect::LoseLife { amount, target } = effect {
            let on_self = match target {
                None | Some(TargetFilter::Controller) | Some(TargetFilter::SourceController) => true,
                Some(_) => false,
            };
            if on_self {
                loss = loss.saturating_add(resolve_quantity(ctx.state, amount, ctx.ai_player, source_id).max(0));
            }
        }
    }
    loss > 0 && loss >= ctx.state.players[ctx.ai_player.0 as usize].life
}

/// A creature with flash cast during the OPPONENT's upkeep or draw step, with an
/// empty stack and no enters-the-battlefield trigger, is dominated by casting it
/// at their end step (CR 500.1): same board on the AI's turn, no information
/// given, no sorcery-speed removal window handed over. The mirror of the 1101
/// gate for the AI's own beginning phases.
fn flash_creature_in_opponent_beginning_phase(ctx: &PolicyContext<'_>) -> bool {
    let GameAction::CastSpell { .. } = &ctx.candidate.action else {
        return false;
    };
    if !ctx.state.stack.is_empty()
        || !matches!(ctx.state.phase, Phase::Upkeep | Phase::Draw)
        || turn_decision_maker(ctx.state) == ctx.ai_player
    {
        return false;
    }
    let Some(facts) = ctx.cast_facts() else {
        return false;
    };
    let object = facts.object;
    let is_creature = object.card_types.core_types.contains(&CoreType::Creature);
    let is_spell_card = object.card_types.core_types.contains(&CoreType::Instant)
        || object.card_types.core_types.contains(&CoreType::Sorcery);
    is_creature && !is_spell_card && facts.immediate_etb_triggers.is_empty()
}

/// Keywords whose value is settled AFTER blockers are declared: granting one
/// earlier on the AI's own turn buys nothing and tells the opponent what to
/// expect; granting it in the AI's own upkeep burns the mana of the main phase.
/// Evasion (flying, menace…) and haste are deliberately NOT here — they matter
/// before blocks / before attackers.
fn keyword_settles_after_blocks(keyword: &Keyword) -> bool {
    matches!(
        keyword,
        Keyword::Hexproof
            | Keyword::HexproofFrom(_)
            | Keyword::Shroud
            | Keyword::Indestructible
            | Keyword::FirstStrike
            | Keyword::DoubleStrike
            | Keyword::Deathtouch
            | Keyword::Lifelink
            | Keyword::Trample
    )
}

/// An activated ability with a MANA cost whose whole effect is an until-end-of-
/// turn grant of after-blocks keywords, used outside the windows where the grant
/// can matter: with an empty stack, before blockers are declared (CR 509) on
/// either turn, or in a main/end step. Allowed with a live stack (a response),
/// after blocks and at combat damage.
fn mana_grant_outside_window(ctx: &PolicyContext<'_>) -> bool {
    let GameAction::ActivateAbility {
        source_id,
        ability_index,
        ..
    } = &ctx.candidate.action
    else {
        return false;
    };
    let Some(ability) = ctx
        .state
        .objects
        .get(source_id)
        .and_then(|object| object.abilities.get(*ability_index))
    else {
        return false;
    };
    let cost = ability.cost.as_ref();
    if !cost_has_category(cost, CostCategory::ManaOnly)
        || cost_has_category(cost, CostCategory::SacrificesPermanent)
        || cost_has_category(cost, CostCategory::PaysLoyalty)
    {
        return false;
    }
    let effects = ctx.effects();
    if effects.is_empty() {
        return false;
    }
    for effect in effects {
        let Effect::GenericEffect {
            static_abilities,
            duration: Some(Duration::UntilEndOfTurn),
            ..
        } = effect
        else {
            return false;
        };
        let Some(changes) = keyword_changes(static_abilities) else {
            return false;
        };
        if !changes.iter().all(|change| match change {
            KeywordChange::Add(keyword) => keyword_settles_after_blocks(keyword),
            KeywordChange::Remove(_) => false,
        }) {
            return false;
        }
    }
    let facts = TacticalFacts::derive(ctx.state, ctx.ai_player);
    if facts.live_stack_response {
        return false;
    }
    !matches!(
        facts.window,
        TacticalWindow::CombatAfterBlocks | TacticalWindow::CombatDamage
    )
}

// ── target gates ─────────────────────────────────────────────────────────────

/// An Aura whose statics only add keywords, aimed at a creature that already has
/// every one of them (Mystic Veil's hexproof on Paradise Druid).
fn aura_keywords_already_present(ctx: &PolicyContext<'_>, object_id: ObjectId) -> bool {
    let Some(source) = ctx.source_object() else {
        return false;
    };
    aura_adds_nothing_to(ctx.state, source, object_id)
}

/// CR 303.4: an Aura spell targets through its Enchant keyword, not through a
/// spell effect, and with ONE legal host the engine binds it at announcement —
/// no `ChooseTarget` step ever runs. So the host check has to run on the cast
/// too: every legal host already has every keyword the Aura would add.
fn aura_cast_hosts_all_redundant(ctx: &PolicyContext<'_>) -> bool {
    let GameAction::CastSpell { object_id, .. } = &ctx.candidate.action else {
        return false;
    };
    let Some(source) = ctx.state.objects.get(object_id) else {
        return false;
    };
    let Some(filter) = source.keywords.iter().find_map(|keyword| match keyword {
        Keyword::Enchant(filter) => Some(filter),
        _ => None,
    }) else {
        return false;
    };
    let hosts = find_legal_targets(ctx.state, filter, ctx.ai_player, *object_id);
    !hosts.is_empty()
        && hosts.iter().all(|host| match host {
            TargetRef::Object(id) => aura_adds_nothing_to(ctx.state, source, *id),
            TargetRef::Player(_) => false,
        })
}

fn is_aura(source: &engine::game::game_object::GameObject) -> bool {
    source
        .card_types
        .subtypes
        .iter()
        .any(|subtype| subtype == "Aura")
        || source
            .keywords
            .iter()
            .any(|keyword| matches!(keyword, Keyword::Enchant(_)))
}

fn aura_adds_nothing_to(state: &GameState, source: &engine::game::game_object::GameObject, object_id: ObjectId) -> bool {
    if !is_aura(source) {
        return false;
    }
    let Some(target) = state.objects.get(&object_id) else {
        return false;
    };
    let mut saw_keyword = false;
    for stat in source.static_definitions.iter_unchecked() {
        if stat.condition.is_some() {
            return false;
        }
        for modification in &stat.modifications {
            match modification {
                ContinuousModification::AddKeyword { keyword } => {
                    saw_keyword = true;
                    if !target.has_keyword(keyword) {
                        return false;
                    }
                }
                _ => return false,
            }
        }
    }
    // The Aura must not do anything else (a trigger or a replacement is a payoff
    // this gate cannot price).
    saw_keyword
        && source.trigger_definitions.iter_unchecked().next().is_none()
        && source.replacement_definitions.iter_unchecked().next().is_none()
}

/// Removal whose rider lets the target's controller search for a basic land
/// (Assassin's Trophy, Beast Within's cousins) aimed at a BASIC LAND: the
/// opponent gets the same card back, untapped, and the AI spent a removal spell.
fn land_removal_with_basic_compensation(ctx: &PolicyContext<'_>, object_id: ObjectId) -> bool {
    let Some(target) = ctx.state.objects.get(&object_id) else {
        return false;
    };
    if !target.card_types.core_types.contains(&CoreType::Land)
        || !target.card_types.supertypes.contains(&Supertype::Basic)
    {
        return false;
    }
    let effects = ctx.effects();
    let removes = effects.iter().any(|effect| {
        matches!(
            effect,
            Effect::Destroy { .. }
                | Effect::ChangeZone {
                    destination: Zone::Exile | Zone::Graveyard | Zone::Library,
                    ..
                }
        )
    });
    let compensates = effects.iter().any(|effect| {
        matches!(
            effect,
            Effect::SearchLibrary {
                target_player: Some(TargetFilter::ParentTargetController),
                ..
            }
        )
    });
    removes && compensates
}

/// Damage that does not kill the chosen creature while another legal opposing
/// creature target WOULD die (Chain Lightning at a 6/6 beside a 2/2). Only the
/// AI's own fixed damage is read; the player as a target is left to scoring.
fn nonlethal_burn_target_with_lethal_alternative(ctx: &PolicyContext<'_>, object_id: ObjectId) -> bool {
    let Some(target) = ctx.state.objects.get(&object_id) else {
        return false;
    };
    if !target.card_types.core_types.contains(&CoreType::Creature)
        || !players::is_opponent(ctx.state, ctx.ai_player, target.controller)
    {
        return false;
    }
    let effects = ctx.effects();
    if !effects
        .iter()
        .all(|effect| matches!(effect, Effect::DealDamage { .. }))
    {
        return false;
    }
    // Only a fully modelled, non-lethal outcome on the chosen target is a proof.
    let PendingDamage::Dealt(outcome) = pending_damage_to_object(ctx, object_id, target) else {
        return false;
    };
    if outcome_is_lethal(target, &outcome) {
        return false;
    }
    let Some(legal) = current_target_selection_targets(ctx.state) else {
        return false;
    };
    legal.iter().any(|other| match other {
        TargetRef::Object(id) if *id != object_id => ctx.state.objects.get(id).is_some_and(|object| {
            object.card_types.core_types.contains(&CoreType::Creature)
                && players::is_opponent(ctx.state, ctx.ai_player, object.controller)
                && matches!(
                    pending_damage_to_object(ctx, *id, object),
                    PendingDamage::Dealt(other_outcome) if outcome_is_lethal(object, &other_outcome)
                )
        }),
        _ => false,
    })
}

/// A beneficial-only effect (counters, pump, keywords, life, cards) aimed at an
/// opponent's object while a legal target of the AI's own exists. The Aura case
/// was already rejected in `anti_self_harm`; triggers (Optimistic Scavenger) and
/// activated abilities were not.
fn beneficial_target_on_opponent_with_own_alternative(ctx: &PolicyContext<'_>, object_id: ObjectId) -> bool {
    let Some(target) = ctx.state.objects.get(&object_id) else {
        return false;
    };
    if !players::is_opponent(ctx.state, ctx.ai_player, target.controller) {
        return false;
    }
    let effects = ctx.effects();
    if effects.is_empty()
        || !effects
            .iter()
            .all(|effect| matches!(effect_polarity(effect), EffectPolarity::Beneficial))
        || !is_spell_beneficial(ctx)
    {
        return false;
    }
    // Control changes and exchanges are beneficial FOR the AI by taking the
    // opponent's object — not this gate's shape.
    if effects.iter().any(|effect| {
        matches!(
            effect,
            Effect::GainControl { .. } | Effect::GainControlAll { .. } | Effect::ExchangeControl { .. }
        )
    }) {
        return false;
    }
    let Some(legal) = current_target_selection_targets(ctx.state) else {
        return false;
    };
    legal.iter().any(|other| match other {
        TargetRef::Object(id) => ctx
            .state
            .objects
            .get(id)
            .is_some_and(|object| object.controller == ctx.ai_player),
        TargetRef::Player(_) => false,
    })
}
