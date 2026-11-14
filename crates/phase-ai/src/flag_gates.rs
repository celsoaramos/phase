//! Proof-backed vetoes for plays MagicFinder players flagged as "the bot
//! played badly" (2026-10-22). Each case was confirmed on the saved board of
//! the report, and each was a play the AI already scored LOW or near-tied:
//! the final choice is sampled, so only a `Reject` keeps it from happening
//! (see `tactical_gate`'s layering rule). Every predicate here fails open —
//! anything it cannot prove leaves the candidate to normal scoring.
//!
//! * a harmful targeted spell whose every legal target is the AI's own
//!   (Unholy Heat with only the AI's creatures on the board);
//! * an "unless its controller pays {N}" counter when every opposing target's
//!   controller has N untapped (Daze / Spell Pierce into open mana);
//! * a pure until-end-of-turn protection spell with nothing to protect from
//!   (Brave the Elements in response to its own spell, opponent creatureless);
//! * an "enchant player" Aura that gives the enchanted player an extra phase
//!   or turn, aimed at an opponent (Shadow of the Second Sun);
//! * a "return a land you control" ETB returning the land that just entered
//!   while another land could go (the bounce-land loop).
//!
//! Second batch (reports of 23–24/10):
//!
//! * "pay any amount of {E}, deal that much damage to that creature" paid 1 of
//!   12 energy into a 6-toughness creature (Harnessed Lightning) — the amount
//!   prompt was answered by its minimum. Since 26/10 the same spell is also
//!   aimed at a creature it can kill, cast only when one exists, and pays
//!   nothing into a creature that survives (or into the AI's own);
//! * a creature Aura that only helps the enchanted creature (Rancor) cast with
//!   no own creature on the board, so its only legal target was the opponent's;
//! * "target creature gains haste until end of turn" activated every turn on a
//!   creature that could already attack (Hanweir Battlements).

use engine::game::casting::activated_ability_definitions;
use engine::game::game_object::GameObject;
use engine::game::players::is_opponent;
use engine::game::targeting::find_legal_targets;
use engine::types::ability::{
    AbilityCost, AbilityDefinition, AbilityKind, ContinuousModification, ControllerRef, Duration,
    Effect, EffectKind, ExtraPhaseRecipient, FilterProp, QuantityExpr, QuantityRef,
    ResolvedAbility, StaticDefinition, TargetFilter, TargetRef, UnlessPayModifier,
};
use engine::types::actions::GameAction;
use engine::types::card_type::CoreType;
use engine::types::game_state::{GameState, PayableResource, StackEntryKind, WaitingFor};
use engine::types::identifiers::ObjectId;
use engine::types::keywords::Keyword;
use engine::types::phase::Phase;
use engine::types::player::PlayerId;
use engine::types::triggers::TriggerMode;
use engine::types::zones::Zone;

use crate::cast_facts::collect_definition_effects;
use crate::policies::context::PolicyContext;
use crate::policies::effect_classify::{effect_polarity, extract_target_filter, EffectPolarity};
use crate::zone_eval::available_mana;

/// True when one of the field-report vetoes proves the candidate useless or
/// self-harming.
pub(crate) fn rejects(ctx: &PolicyContext<'_>) -> bool {
    match &ctx.candidate.action {
        GameAction::CastSpell { .. } => {
            harmful_spell_reaches_only_own(ctx)
                || soft_counter_is_payable_at_cast(ctx)
                || protection_spell_has_nothing_to_stop(ctx)
                || boon_aura_reaches_only_foreign(ctx)
                || energy_burn_has_nothing_to_kill(ctx)
        }
        GameAction::ChooseTarget {
            target: Some(target),
        } => target_is_rejected(ctx, target),
        GameAction::SelectTargets { targets } => {
            targets.iter().any(|target| target_is_rejected(ctx, target))
        }
        GameAction::SelectCards { cards } => bounce_land_returns_itself(ctx, cards),
        GameAction::SubmitPayAmount { amount } => energy_pay_amount_is_wasted(ctx, *amount),
        GameAction::ActivateAbility {
            source_id,
            ability_index,
            ..
        } => haste_grant_has_no_beneficiary(ctx, *source_id, *ability_index),
        _ => false,
    }
}

fn target_is_rejected(ctx: &PolicyContext<'_>, target: &TargetRef) -> bool {
    soft_counter_target_is_payable(ctx, target)
        || boon_aura_targets_opponent(ctx, target)
        || boon_aura_targets_foreign_creature(ctx, target)
        || haste_grant_target_needs_no_haste(ctx, target)
        || energy_burn_target_misses_kill(ctx, target)
}

/// The instant/sorcery being cast and its root spell ability.
fn cast_spell_definition<'a>(
    ctx: &PolicyContext<'a>,
) -> Option<(&'a GameObject, &'a AbilityDefinition)> {
    let source = ctx.source_object()?;
    let types = &source.card_types.core_types;
    if !types.contains(&CoreType::Instant) && !types.contains(&CoreType::Sorcery) {
        return None;
    }
    let mut spells = source
        .abilities
        .iter()
        .filter(|ability| ability.kind == AbilityKind::Spell);
    let root = spells.next()?;
    // Modal or split payloads carry several roots; their choice is not ours to
    // prove here.
    if spells.next().is_some() {
        return None;
    }
    Some((source, root))
}

// ---------------------------------------------------------------------------
// Harmful spell with only the AI's own permanents as legal targets
// ---------------------------------------------------------------------------

/// CR 601.2c + CR 115.1: a spell whose whole payload is harmful (damage,
/// destroy, exile…) and whose every legal target is the AI or a permanent the
/// AI controls can only hurt the AI. Field report: Unholy Heat with the
/// opponent creatureless — the AI killed its own Insectile Aberration.
///
/// Fails open when the payload has any non-harmful part, when a legal target
/// is foreign, or when an own target carries a leaves/dies trigger (a
/// deliberate self-kill for value).
fn harmful_spell_reaches_only_own(ctx: &PolicyContext<'_>) -> bool {
    let Some((source, root)) = cast_spell_definition(ctx) else {
        return false;
    };
    let effects = collect_definition_effects(root);
    if effects.is_empty()
        || matches!(root.effect.as_ref(), Effect::Counter { .. })
        || !effects
            .iter()
            .all(|effect| matches!(effect_polarity(effect), EffectPolarity::Harmful))
    {
        return false;
    }
    let Some(filter) = extract_target_filter(root.effect.as_ref()) else {
        return false;
    };
    let legal = find_legal_targets(ctx.state, filter, ctx.ai_player, source.id);
    !legal.is_empty()
        && legal.iter().all(|target| match target {
            TargetRef::Player(player) => *player == ctx.ai_player,
            TargetRef::Object(id) => ctx.state.objects.get(id).is_some_and(|object| {
                object.controller == ctx.ai_player
                    && object.zone == Zone::Battlefield
                    && !object
                        .trigger_definitions
                        .iter_unchecked()
                        .any(|trigger| trigger.definition.mode == TriggerMode::ChangesZone)
            }),
        })
}

// ---------------------------------------------------------------------------
// "Counter target spell unless its controller pays {N}" into open mana
// ---------------------------------------------------------------------------

/// The generic tax of a soft counter ("unless its controller pays {N}"), when
/// the payload is exactly that shape.
fn soft_counter_tax(effect: &Effect, unless: Option<&UnlessPayModifier>) -> Option<u32> {
    if !matches!(effect, Effect::Counter { .. }) {
        return None;
    }
    let unless = unless?;
    if !matches!(unless.payer, TargetFilter::ParentTargetController) {
        return None;
    }
    let AbilityCost::Mana { cost } = &unless.cost else {
        return None;
    };
    let tax = cost.mana_value();
    (tax > 0).then_some(tax)
}

/// CR 118.12: the opponent controlling `id` (a spell on the stack) has at
/// least `tax` untapped mana sources (or floating mana) right now.
fn opposing_controller_can_pay(
    state: &GameState,
    ai: PlayerId,
    id: ObjectId,
    tax: u32,
) -> Option<bool> {
    let entry = state.stack.iter().find(|entry| entry.id == id)?;
    if !matches!(entry.kind, StackEntryKind::Spell { .. })
        || !is_opponent(state, ai, entry.controller)
    {
        return None;
    }
    Some(available_mana(state, entry.controller) >= tax)
}

/// Field report (twice, Izzet Delver challenger): Daze cast into an opponent
/// with one and two untapped Forests. The AI scored the cast far above passing
/// (11.9 vs 1.2; 31.6 vs 14.3) — it never read the tax. Rejected only when
/// EVERY opposing spell it could target belongs to a player who can pay.
fn soft_counter_is_payable_at_cast(ctx: &PolicyContext<'_>) -> bool {
    let Some((source, root)) = cast_spell_definition(ctx) else {
        return false;
    };
    let Some(tax) = soft_counter_tax(root.effect.as_ref(), root.unless_pay.as_ref()) else {
        return false;
    };
    let Effect::Counter { target, .. } = root.effect.as_ref() else {
        return false;
    };
    let verdicts: Vec<bool> = find_legal_targets(ctx.state, target, ctx.ai_player, source.id)
        .iter()
        .filter_map(|target| match target {
            TargetRef::Object(id) => {
                opposing_controller_can_pay(ctx.state, ctx.ai_player, *id, tax)
            }
            TargetRef::Player(_) => None,
        })
        .collect();
    !verdicts.is_empty() && verdicts.iter().all(|can_pay| *can_pay)
}

fn soft_counter_target_is_payable(ctx: &PolicyContext<'_>, target: &TargetRef) -> bool {
    let TargetRef::Object(id) = target else {
        return false;
    };
    let WaitingFor::TargetSelection { pending_cast, .. } = &ctx.decision.waiting_for else {
        return false;
    };
    let ability = &pending_cast.ability;
    if ability.controller != ctx.ai_player {
        return false;
    }
    let Some(tax) = soft_counter_tax(&ability.effect, ability.unless_pay.as_ref()) else {
        return false;
    };
    opposing_controller_can_pay(ctx.state, ctx.ai_player, *id, tax).unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Until-end-of-turn protection with nothing to protect from
// ---------------------------------------------------------------------------

fn is_protective_keyword(keyword: &Keyword) -> bool {
    matches!(
        keyword,
        Keyword::Protection(_)
            | Keyword::Hexproof
            | Keyword::HexproofFrom(_)
            | Keyword::Shroud
            | Keyword::Indestructible
    )
}

/// A grant of only protective keywords to the AI's own permanents, ending at
/// cleanup.
fn is_own_protection_grant(effect: &Effect) -> bool {
    let Effect::GenericEffect {
        static_abilities,
        duration,
        ..
    } = effect
    else {
        return false;
    };
    matches!(duration, Some(Duration::UntilEndOfTurn))
        && !static_abilities.is_empty()
        && static_abilities.iter().all(|definition| {
            matches!(
                &definition.affected,
                Some(TargetFilter::Typed(typed))
                    if typed.controller == Some(ControllerRef::You)
            ) && !definition.modifications.is_empty()
                && definition.modifications.iter().all(|modification| {
                    matches!(modification,
                        ContinuousModification::AddKeyword { keyword } if is_protective_keyword(keyword))
                })
        })
}

/// CR 702.16 + CR 514.2: protection that ends this turn only matters against
/// something this turn — an opposing spell or ability on the stack, or an
/// opposing creature (combat). With neither, the card is spent for nothing.
/// Field report: Brave the Elements cast in response to the AI's OWN Field
/// Marshal with the opponent creatureless (scored 33.4 vs 30.3 for passing).
fn protection_spell_has_nothing_to_stop(ctx: &PolicyContext<'_>) -> bool {
    let Some((_, root)) = cast_spell_definition(ctx) else {
        return false;
    };
    let effects = collect_definition_effects(root);
    let mut grants = false;
    for effect in &effects {
        match effect {
            Effect::Choose { .. } => {}
            effect if is_own_protection_grant(effect) => grants = true,
            _ => return false,
        }
    }
    if !grants {
        return false;
    }
    let state = ctx.state;
    let foreign_stack = state
        .stack
        .iter()
        .any(|entry| is_opponent(state, ctx.ai_player, entry.controller));
    let foreign_creature = state.battlefield.iter().any(|id| {
        state.objects.get(id).is_some_and(|object| {
            is_opponent(state, ctx.ai_player, object.controller)
                && object.card_types.core_types.contains(&CoreType::Creature)
        })
    });
    !foreign_stack && !foreign_creature
}

// ---------------------------------------------------------------------------
// "Enchant player" Aura that hands its player an extra phase/turn
// ---------------------------------------------------------------------------

/// The Aura gives the ENCHANTED player an extra phase or turn and nothing
/// else (Shadow of the Second Sun: "At the beginning of each of enchanted
/// player's postcombat main phases, there is an additional beginning phase").
fn aura_is_boon_to_enchanted_player(aura: &GameObject) -> bool {
    let enchants_player = aura
        .keywords
        .iter()
        .any(|keyword| matches!(keyword, Keyword::Enchant(TargetFilter::Player)));
    if !enchants_player || aura.trigger_definitions.is_empty() {
        return false;
    }
    aura.trigger_definitions.iter_unchecked().all(|trigger| {
        let definition = &trigger.definition;
        matches!(definition.valid_target, Some(TargetFilter::AttachedTo))
            && definition.execute.as_ref().is_some_and(|execute| {
                matches!(
                    execute.effect.as_ref(),
                    Effect::AdditionalPhase {
                        recipient: ExtraPhaseRecipient::NoPlayer
                            | ExtraPhaseRecipient::TriggeringPlayer,
                        ..
                    } | Effect::ExtraTurn { .. }
                )
            })
    })
}

/// CR 303.4a: an Aura's enchant target is chosen as it is cast. Field report:
/// the AI enchanted the OPPONENT with Shadow of the Second Sun — an extra
/// untap/upkeep/draw for them every turn. Rejected only when the AI itself is
/// a legal choice in the same selection.
fn boon_aura_targets_opponent(ctx: &PolicyContext<'_>, target: &TargetRef) -> bool {
    let TargetRef::Player(player) = target else {
        return false;
    };
    if !is_opponent(ctx.state, ctx.ai_player, *player) {
        return false;
    }
    let WaitingFor::TargetSelection {
        pending_cast,
        target_slots,
        ..
    } = &ctx.decision.waiting_for
    else {
        return false;
    };
    let self_is_legal = target_slots.iter().any(|slot| {
        slot.legal_targets
            .contains(&TargetRef::Player(ctx.ai_player))
    });
    self_is_legal
        && ctx
            .state
            .objects
            .get(&pending_cast.object_id)
            .is_some_and(|aura| {
                aura.card_types
                    .subtypes
                    .iter()
                    .any(|subtype| subtype == "Aura")
                    && aura_is_boon_to_enchanted_player(aura)
            })
}

// ---------------------------------------------------------------------------
// Bounce land returning itself
// ---------------------------------------------------------------------------

/// CR 608.2c: "return a land you control to its owner's hand". Returning the
/// bounce land that just entered loops it back to hand every turn; the
/// `SelfBounceTargetPolicy` already scores it down, but as a score it is
/// sampled. Field report: turn 10, the AI's fifth turn, one Mountain on the
/// battlefield and two Izzet Boilerworks in hand. Rejected only while enough
/// OTHER own lands exist to fill the selection.
fn bounce_land_returns_itself(ctx: &PolicyContext<'_>, cards: &[ObjectId]) -> bool {
    let WaitingFor::EffectZoneChoice {
        player,
        source_id,
        effect_kind: EffectKind::ChangeZone,
        zone: Zone::Battlefield,
        destination: Some(Zone::Hand),
        cards: pool,
        ..
    } = &ctx.decision.waiting_for
    else {
        return false;
    };
    if *player != ctx.ai_player || !cards.contains(source_id) {
        return false;
    }
    let own_land = |id: &ObjectId| {
        ctx.state.objects.get(id).is_some_and(|object| {
            object.controller == ctx.ai_player
                && object.card_types.core_types.contains(&CoreType::Land)
        })
    };
    if !pool.iter().all(own_land) {
        return false;
    }
    let alternatives = pool.iter().filter(|id| *id != source_id).count();
    alternatives >= cards.len()
}

// ---------------------------------------------------------------------------
// "Pay any amount of {E}; deal that much damage" paid below lethal
// ---------------------------------------------------------------------------

/// `true` for the two steps that make "pay any amount of {E}, ~ deals that
/// much damage to that creature": a `PayCost(PayEnergy X)` immediately followed
/// by `DealDamage(EventContextAmount)` at the chain's target.
fn is_energy_burn_payment(payment: &Effect, next: Option<&Effect>) -> bool {
    matches!(
        payment,
        Effect::PayCost {
            cost: AbilityCost::PayEnergy {
                amount: QuantityExpr::Ref {
                    qty: QuantityRef::Variable { .. },
                },
            },
            ..
        }
    ) && matches!(
        next,
        Some(Effect::DealDamage {
            amount: QuantityExpr::Ref {
                qty: QuantityRef::EventContextAmount,
            },
            target: TargetFilter::TriggeringSource,
            ..
        })
    )
}

/// CR 107.14 + CR 120.3: "you get {E}{E}{E}, then you may pay any amount of
/// {E}. ~ deals that much damage to that creature" (Harnessed Lightning). Walks
/// a chain of effects in resolution order and returns the energy the chain
/// itself grants before the payment, or `None` when the chain has no such
/// payment. Shared by the definition (cast) and resolved (target, payment)
/// views of the same spell.
fn energy_burn_grant<'a>(mut steps: impl Iterator<Item = &'a Effect>) -> Option<u32> {
    let mut granted = 0u32;
    let mut current = steps.next();
    while let Some(effect) = current {
        let next = steps.next();
        if is_energy_burn_payment(effect, next) {
            return Some(granted);
        }
        if let Effect::GainEnergy {
            amount: QuantityExpr::Fixed { value },
        } = effect
        {
            granted = granted.saturating_add(u32::try_from(*value).unwrap_or(0));
        }
        current = next;
    }
    None
}

fn resolved_chain(root: &ResolvedAbility) -> impl Iterator<Item = &Effect> {
    std::iter::successors(Some(root), |ability| ability.sub_ability.as_deref())
        .map(|ability| &ability.effect)
}

fn definition_chain(root: &AbilityDefinition) -> impl Iterator<Item = &Effect> {
    std::iter::successors(Some(root), |ability| ability.sub_ability.as_deref())
        .map(|ability| ability.effect.as_ref())
}

/// The creature a resolving energy-burn chain is pointed at (the chain root's
/// target). `None` for every other shape.
fn damage_per_energy_target(root: &ResolvedAbility) -> Option<TargetRef> {
    energy_burn_grant(resolved_chain(root))?;
    root.targets
        .first()
        .or_else(|| root.context.chain_root_targets.first())
        .cloned()
}

/// What paying up to `budget` energy into `id` can do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EnergyBurnOutcome {
    /// CR 704.5g: this much damage is lethal and within the budget.
    Kills(u32),
    /// The creature survives every affordable amount: indestructible
    /// (CR 702.12b) or tougher than the budget.
    Survives,
    /// Not a creature on the battlefield, or already at lethal damage.
    Unknown,
}

fn energy_burn_outcome(state: &GameState, id: ObjectId, budget: u32) -> EnergyBurnOutcome {
    let Some(creature) = state.objects.get(&id) else {
        return EnergyBurnOutcome::Unknown;
    };
    if creature.zone != Zone::Battlefield
        || !creature.card_types.core_types.contains(&CoreType::Creature)
    {
        return EnergyBurnOutcome::Unknown;
    }
    // CR 702.12b: lethal damage doesn't destroy an indestructible creature.
    if creature.has_keyword(&Keyword::Indestructible) {
        return EnergyBurnOutcome::Survives;
    }
    let remaining = creature.toughness.unwrap_or(0) - creature.damage_marked as i32;
    let Ok(lethal) = u32::try_from(remaining) else {
        return EnergyBurnOutcome::Unknown;
    };
    if lethal == 0 {
        EnergyBurnOutcome::Unknown
    } else if lethal > budget {
        EnergyBurnOutcome::Survives
    } else {
        EnergyBurnOutcome::Kills(lethal)
    }
}

/// CR 510.1 + CR 120.6: `id` is blocking or blocked by a creature `ai`
/// controls, so combat damage adds to whatever the spell deals — a
/// non-lethal amount can still finish it.
fn shares_combat_with(state: &GameState, ai: PlayerId, id: ObjectId) -> bool {
    let Some(combat) = state.combat.as_ref() else {
        return false;
    };
    let own = |other: &ObjectId| {
        state
            .objects
            .get(other)
            .is_some_and(|object| object.controller == ai)
    };
    combat
        .blocker_assignments
        .get(&id)
        .is_some_and(|blockers| blockers.iter().any(own))
        || combat
            .blocker_to_attacker
            .get(&id)
            .is_some_and(|attackers| attackers.iter().any(own))
}

fn energy_of(state: &GameState, player: PlayerId) -> u32 {
    state
        .players
        .iter()
        .find(|candidate| candidate.id == player)
        .map_or(0, |candidate| candidate.energy)
}

/// An opposing creature the energy-burn spell kills with `budget` energy.
fn energy_burn_kills_opponent(
    state: &GameState,
    ai: PlayerId,
    target: &TargetRef,
    budget: u32,
) -> bool {
    let TargetRef::Object(id) = target else {
        return false;
    };
    state
        .objects
        .get(id)
        .is_some_and(|object| is_opponent(state, ai, object.controller))
        && matches!(
            energy_burn_outcome(state, *id, budget),
            EnergyBurnOutcome::Kills(_)
        )
}

/// CR 107.14 + CR 120.3: the amount paid IS the damage. Field reports (24/10,
/// Kaladesh Energy challenger): Harnessed Lightning on an 18/6 with 12 energy
/// in the pool paid 1 — every amount was scored 0.1 apart and the prompt's
/// default is its minimum. Rejected:
/// * at an opposing creature that can die, any amount but the lethal one;
/// * at an opposing creature that survives every affordable amount and isn't
///   in combat with the AI's creatures, any amount above the minimum — the
///   damage does nothing and the energy is gone;
/// * at the AI's own creature, any amount above the minimum (the spell was
///   cast for its energy; paying would only hurt the AI's creature).
fn energy_pay_amount_is_wasted(ctx: &PolicyContext<'_>, amount: u32) -> bool {
    let WaitingFor::PayAmountChoice {
        player,
        resource: PayableResource::Energy,
        min,
        max,
        source_id,
        ..
    } = &ctx.decision.waiting_for
    else {
        return false;
    };
    if *player != ctx.ai_player {
        return false;
    }
    let Some(entry) = ctx.state.resolving_stack_entry.as_ref() else {
        return false;
    };
    if entry.id != *source_id || entry.controller != ctx.ai_player {
        return false;
    }
    let StackEntryKind::Spell {
        ability: Some(root),
        ..
    } = &entry.kind
    else {
        return false;
    };
    let Some(TargetRef::Object(target)) = damage_per_energy_target(root) else {
        return false;
    };
    let Some(creature) = ctx.state.objects.get(&target) else {
        return false;
    };
    if creature.controller == ctx.ai_player {
        return amount > *min;
    }
    if !is_opponent(ctx.state, ctx.ai_player, creature.controller) {
        return false;
    }
    match energy_burn_outcome(ctx.state, target, *max) {
        EnergyBurnOutcome::Kills(lethal) if lethal >= *min => amount != lethal,
        EnergyBurnOutcome::Survives if !shares_combat_with(ctx.state, ctx.ai_player, target) => {
            amount > *min
        }
        _ => false,
    }
}

/// CR 601.2c: the target is chosen before the spell's own energy arrives, so
/// the budget is the pool plus what the chain grants. Rejected: a target the
/// spell can't kill while another legal opposing creature would die.
fn energy_burn_target_misses_kill(ctx: &PolicyContext<'_>, target: &TargetRef) -> bool {
    let WaitingFor::TargetSelection {
        pending_cast,
        target_slots,
        ..
    } = &ctx.decision.waiting_for
    else {
        return false;
    };
    let ability = &pending_cast.ability;
    if ability.controller != ctx.ai_player {
        return false;
    }
    let Some(granted) = energy_burn_grant(resolved_chain(ability)) else {
        return false;
    };
    let budget = energy_of(ctx.state, ctx.ai_player).saturating_add(granted);
    let kills = |candidate: &TargetRef| {
        energy_burn_kills_opponent(ctx.state, ctx.ai_player, candidate, budget)
    };
    !kills(target)
        && target_slots
            .iter()
            .flat_map(|slot| slot.legal_targets.iter())
            .any(kills)
}

/// Harnessed Lightning is removal that also pays three energy. Cast with no
/// opposing creature it can kill, the card buys three energy and nothing
/// else. Rejected only when no legal opposing creature dies to the pool plus
/// the spell's own energy and none is in combat with the AI's creatures.
fn energy_burn_has_nothing_to_kill(ctx: &PolicyContext<'_>) -> bool {
    let Some((source, root)) = cast_spell_definition(ctx) else {
        return false;
    };
    let Some(granted) = energy_burn_grant(definition_chain(root)) else {
        return false;
    };
    let filter = match root.effect.as_ref() {
        Effect::TargetOnly { target } => target,
        effect => match extract_target_filter(effect) {
            Some(filter) => filter,
            None => return false,
        },
    };
    let budget = energy_of(ctx.state, ctx.ai_player).saturating_add(granted);
    let legal = find_legal_targets(ctx.state, filter, ctx.ai_player, source.id);
    !legal.iter().any(|target| {
        energy_burn_kills_opponent(ctx.state, ctx.ai_player, target, budget)
            || matches!(target, TargetRef::Object(id)
                if shares_combat_with(ctx.state, ctx.ai_player, *id))
    })
}

// ---------------------------------------------------------------------------
// Creature Aura that only helps its creature, on the opponent's creature
// ---------------------------------------------------------------------------

/// A modification that can only help the creature carrying it.
fn modification_is_boon(modification: &ContinuousModification) -> bool {
    match modification {
        ContinuousModification::AddPower { value }
        | ContinuousModification::AddToughness { value } => *value >= 0,
        ContinuousModification::AddKeyword { keyword } => !matches!(keyword, Keyword::Defender),
        _ => false,
    }
}

/// The Aura enchants a creature and every static it carries only helps the
/// enchanted creature (Rancor: "+2/+0 and has trample"). Auras with any other
/// static (a drawback, a tax on the controller) fail open.
fn aura_is_boon_to_enchanted_creature(aura: &GameObject) -> bool {
    if !aura
        .card_types
        .subtypes
        .iter()
        .any(|subtype| subtype == "Aura")
    {
        return false;
    }
    let enchants_creature = aura.keywords.iter().any(|keyword| {
        matches!(keyword, Keyword::Enchant(TargetFilter::Typed(typed))
            if typed.type_filters.iter().any(|filter| matches!(filter, engine::types::ability::TypeFilter::Creature)))
    });
    if !enchants_creature {
        return false;
    }
    // "Enchanted creature gets…" parses as the typed filter with the
    // `EnchantedBy` property; the bare `AttachedTo` form is kept for parity.
    let affects_enchanted = |definition: &StaticDefinition| match &definition.affected {
        Some(TargetFilter::AttachedTo) => true,
        Some(TargetFilter::Typed(typed)) => typed
            .properties
            .iter()
            .any(|property| matches!(property, FilterProp::EnchantedBy)),
        _ => false,
    };
    let statics: Vec<&StaticDefinition> = aura.static_definitions.iter_unchecked().collect();
    !statics.is_empty()
        && statics.iter().all(|definition| {
            affects_enchanted(definition)
                && !definition.modifications.is_empty()
                && definition.modifications.iter().all(modification_is_boon)
        })
}

/// The Aura's enchant filter, when it is a typed one.
fn aura_enchant_filter(aura: &GameObject) -> Option<&TargetFilter> {
    aura.keywords.iter().find_map(|keyword| match keyword {
        Keyword::Enchant(filter) => Some(filter),
        _ => None,
    })
}

/// CR 303.4a + CR 601.2c: an Aura that only helps its creature, cast when every
/// creature it could enchant belongs to an opponent, can only help them. Field
/// report (23/10): Rancor cast with the AI creatureless — its only legal
/// targets were the opponent's Guttersnipe and Sneaky Snacker.
fn boon_aura_reaches_only_foreign(ctx: &PolicyContext<'_>) -> bool {
    let Some(aura) = ctx.source_object() else {
        return false;
    };
    if !aura_is_boon_to_enchanted_creature(aura) {
        return false;
    }
    let Some(filter) = aura_enchant_filter(aura) else {
        return false;
    };
    let legal = find_legal_targets(ctx.state, filter, ctx.ai_player, aura.id);
    !legal.is_empty()
        && legal.iter().all(|target| match target {
            TargetRef::Object(id) => ctx
                .state
                .objects
                .get(id)
                .is_some_and(|object| is_opponent(ctx.state, ctx.ai_player, object.controller)),
            TargetRef::Player(_) => false,
        })
}

/// Same Aura, at the target prompt: the opponent's creature chosen while an
/// own creature is a legal choice in the same selection.
fn boon_aura_targets_foreign_creature(ctx: &PolicyContext<'_>, target: &TargetRef) -> bool {
    let TargetRef::Object(id) = target else {
        return false;
    };
    let foreign = ctx
        .state
        .objects
        .get(id)
        .is_some_and(|object| is_opponent(ctx.state, ctx.ai_player, object.controller));
    if !foreign {
        return false;
    }
    let WaitingFor::TargetSelection {
        pending_cast,
        target_slots,
        ..
    } = &ctx.decision.waiting_for
    else {
        return false;
    };
    if pending_cast.ability.controller != ctx.ai_player {
        return false;
    }
    let own_is_legal = target_slots.iter().any(|slot| {
        slot.legal_targets.iter().any(|legal| match legal {
            TargetRef::Object(legal_id) => ctx
                .state
                .objects
                .get(legal_id)
                .is_some_and(|object| object.controller == ctx.ai_player),
            TargetRef::Player(_) => false,
        })
    });
    own_is_legal
        && ctx
            .state
            .objects
            .get(&pending_cast.object_id)
            .is_some_and(aura_is_boon_to_enchanted_creature)
}

// ---------------------------------------------------------------------------
// "Target creature gains haste until end of turn" with nothing to hasten
// ---------------------------------------------------------------------------

/// The effect grants haste (and only haste) until end of turn.
fn effect_is_haste_grant(effect: &Effect) -> bool {
    let Effect::GenericEffect {
        static_abilities,
        duration,
        ..
    } = effect
    else {
        return false;
    };
    matches!(duration, Some(Duration::UntilEndOfTurn))
        && !static_abilities.is_empty()
        && static_abilities.iter().all(|definition| {
            !definition.modifications.is_empty()
                && definition.modifications.iter().all(|modification| {
                    matches!(
                        modification,
                        ContinuousModification::AddKeyword {
                            keyword: Keyword::Haste
                        }
                    )
                })
        })
}

/// CR 302.6: haste only matters to a creature that came under the AI's control
/// this turn, and only before attackers are declared on the AI's own turn.
fn haste_window_open(state: &GameState, ai: PlayerId) -> bool {
    state.active_player == ai
        && matches!(
            state.phase,
            Phase::Upkeep | Phase::Draw | Phase::PreCombatMain | Phase::BeginCombat
        )
}

fn creature_needs_haste(state: &GameState, ai: PlayerId, object: &GameObject) -> bool {
    object.controller == ai
        && object.zone == Zone::Battlefield
        && object.card_types.core_types.contains(&CoreType::Creature)
        && !object.has_keyword(&Keyword::Haste)
        && (object.summoning_sick || object.entered_battlefield_turn == Some(state.turn_number))
}

/// Field report (23/10, Amulet Titan challenger): Hanweir Battlements paid
/// {R},{T} every upkeep to give haste to a creature that had been on the
/// battlefield for turns. Rejected at activation when no own creature could use
/// haste right now (none summoning-sick, or the window for attacking this turn
/// has closed).
fn haste_grant_has_no_beneficiary(
    ctx: &PolicyContext<'_>,
    source_id: ObjectId,
    ability_index: usize,
) -> bool {
    let definitions = activated_ability_definitions(ctx.state, source_id);
    let Some((_, definition)) = definitions
        .iter()
        .find(|(index, _)| *index == ability_index)
    else {
        return false;
    };
    if definition.kind != AbilityKind::Activated || !effect_is_haste_grant(&definition.effect) {
        return false;
    }
    if !haste_window_open(ctx.state, ctx.ai_player) {
        return true;
    }
    !ctx.state.battlefield.iter().any(|id| {
        ctx.state
            .objects
            .get(id)
            .is_some_and(|object| creature_needs_haste(ctx.state, ctx.ai_player, object))
    })
}

/// Same grant, at the target prompt: the chosen creature is not one that
/// needs haste (it can already attack, has haste, or is not the AI's).
fn haste_grant_target_needs_no_haste(ctx: &PolicyContext<'_>, target: &TargetRef) -> bool {
    let TargetRef::Object(id) = target else {
        return false;
    };
    let WaitingFor::TargetSelection { pending_cast, .. } = &ctx.decision.waiting_for else {
        return false;
    };
    let ability = &pending_cast.ability;
    if ability.controller != ctx.ai_player || !effect_is_haste_grant(&ability.effect) {
        return false;
    }
    if !haste_window_open(ctx.state, ctx.ai_player) {
        return true;
    }
    !ctx.state
        .objects
        .get(id)
        .is_some_and(|object| creature_needs_haste(ctx.state, ctx.ai_player, object))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::combat_ai::zero_power_attack_is_pointless;
    use crate::config::{create_config, AiDifficulty, Platform};
    use crate::context::AiContext;
    use crate::tactical_gate::gate_candidates;
    use engine::ai_support::{ActionMetadata, AiDecisionContext, CandidateAction, TacticalClass};
    use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
    use engine::game::zones::create_object;
    use engine::types::ability::TypedFilter;
    use engine::types::game_state::{CastPaymentMode, CastingVariant, StackEntry};
    use engine::types::identifiers::CardId;
    use engine::types::keywords::Keyword;
    use engine::types::mana::{ManaColor, ManaCost};
    use engine::types::phase::Phase;

    const UNHOLY_HEAT: &str = "Unholy Heat deals 2 damage to target creature or planeswalker.\nDelirium — Unholy Heat deals 6 damage instead if there are four or more card types among cards in your graveyard.";
    const DAZE: &str = "You may return an Island you control to its owner's hand rather than pay this spell's mana cost.\nCounter target spell unless its controller pays {1}.";
    const BRAVE: &str = "Choose a color. White creatures you control gain protection from the chosen color until end of turn.";
    const SHADOW: &str = "Enchant player\nAt the beginning of each of enchanted player's postcombat main phases, there is an additional beginning phase after this phase. (The beginning phase includes the untap, upkeep, and draw steps.)";
    const BOILERWORKS: &str = "Izzet Boilerworks enters tapped.\nWhen Izzet Boilerworks enters, return a land you control to its owner's hand.\n{T}: Add {U}{R}.";

    /// Does the production gate (with this module hooked in) keep `action`?
    fn kept(state: &GameState, action: GameAction) -> bool {
        let config = create_config(AiDifficulty::VeryHard, Platform::Wasm);
        let context = AiContext::empty(&config.weights);
        let decision = AiDecisionContext {
            waiting_for: state.waiting_for.clone(),
            candidates: Vec::new(),
        };
        let candidate = CandidateAction {
            action,
            metadata: ActionMetadata::for_actor(Some(P0), TacticalClass::Spell),
        };
        !gate_candidates(state, &decision, vec![candidate], P0, &config, &context).is_empty()
    }

    fn cast(state: &GameState, spell: ObjectId) -> GameAction {
        GameAction::CastSpell {
            object_id: spell,
            card_id: state.objects[&spell].card_id,
            targets: Vec::new(),
            payment_mode: CastPaymentMode::Auto,
        }
    }

    fn priority_to_p0(runner: &mut GameRunner, active: PlayerId) {
        let state = runner.state_mut();
        state.active_player = active;
        state.priority_player = P0;
        state.waiting_for = WaitingFor::Priority { player: P0 };
    }

    fn push_stack_spell(state: &mut GameState, controller: PlayerId, name: &str) -> ObjectId {
        let id = create_object(
            state,
            CardId(state.next_object_id),
            controller,
            name.to_string(),
            Zone::Stack,
        );
        let card_id = state.objects[&id].card_id;
        state
            .objects
            .get_mut(&id)
            .unwrap()
            .card_types
            .core_types
            .push(CoreType::Sorcery);
        state.stack.push_back(StackEntry {
            id,
            source_id: id,
            controller,
            kind: StackEntryKind::Spell {
                card_id,
                ability: None,
                casting_variant: CastingVariant::Normal,
                actual_mana_spent: 0,
            },
        });
        id
    }

    // ── Unholy Heat on its own board ───────────────────────────────────────

    fn unholy_heat_board(opponent_creature: bool) -> (GameState, ObjectId) {
        let mut scenario = GameScenario::new();
        scenario.at_phase(Phase::PostCombatMain);
        scenario.add_vanilla(P0, 3, 2);
        scenario.add_vanilla(P0, 3, 3);
        if opponent_creature {
            scenario.add_vanilla(P1, 2, 2);
        }
        let heat = scenario
            .add_spell_to_hand_from_oracle(P0, "Unholy Heat", true, UNHOLY_HEAT)
            .id();
        let mut runner = scenario.build();
        priority_to_p0(&mut runner, P0);
        (runner.state().clone(), heat)
    }

    /// Field report (22/10, Izzet Delver): the opponent had no creature and the
    /// AI cast Unholy Heat — its only legal targets were its own creatures.
    #[test]
    fn rejects_unholy_heat_when_only_own_creatures_are_targets() {
        let (state, heat) = unholy_heat_board(false);
        assert!(!kept(&state, cast(&state, heat)));
    }

    #[test]
    fn keeps_unholy_heat_with_an_opposing_creature() {
        let (state, heat) = unholy_heat_board(true);
        assert!(kept(&state, cast(&state, heat)));
    }

    // ── Daze into open mana ────────────────────────────────────────────────

    fn daze_board(opponent_untapped_lands: usize) -> (GameState, ObjectId) {
        let mut scenario = GameScenario::new();
        scenario.at_phase(Phase::PreCombatMain);
        let daze = scenario
            .add_spell_to_hand_from_oracle(P0, "Daze", true, DAZE)
            .with_mana_cost(ManaCost::Cost {
                generic: 1,
                shards: vec![engine::types::mana::ManaCostShard::Blue],
            })
            .id();
        let mut lands = Vec::new();
        for _ in 0..3 {
            lands.push(scenario.add_basic_land(P1, ManaColor::Green));
        }
        let mut runner = scenario.build();
        let state = runner.state_mut();
        for (index, land) in lands.iter().enumerate() {
            state.objects.get_mut(land).unwrap().tapped = index >= opponent_untapped_lands;
        }
        push_stack_spell(state, P1, "Rampant Growth");
        priority_to_p0(&mut runner, P1);
        (runner.state().clone(), daze)
    }

    /// Field reports (22/10, twice): Daze into one and into two open Forests.
    #[test]
    fn rejects_daze_when_the_target_controller_can_pay() {
        for open in [1, 2] {
            let (state, daze) = daze_board(open);
            assert!(!kept(&state, cast(&state, daze)), "{open} open land(s)");
        }
    }

    #[test]
    fn keeps_daze_when_the_opponent_is_tapped_out() {
        let (state, daze) = daze_board(0);
        assert!(kept(&state, cast(&state, daze)));
    }

    // ── Brave the Elements with nothing to stop ────────────────────────────

    fn brave_board(opponent_creature: bool, opponent_spell: bool) -> (GameState, ObjectId) {
        let mut scenario = GameScenario::new();
        scenario.at_phase(Phase::PreCombatMain);
        scenario.add_vanilla(P0, 2, 2);
        if opponent_creature {
            scenario.add_vanilla(P1, 2, 2);
        }
        let brave = scenario
            .add_spell_to_hand_from_oracle(P0, "Brave the Elements", true, BRAVE)
            .id();
        let mut runner = scenario.build();
        if opponent_spell {
            push_stack_spell(runner.state_mut(), P1, "Lightning Bolt");
        }
        priority_to_p0(&mut runner, P0);
        (runner.state().clone(), brave)
    }

    /// Field report (19/10, mirror): cast in response to the AI's own Field
    /// Marshal with the opponent creatureless.
    #[test]
    fn rejects_protection_with_no_opposing_creature_or_spell() {
        let (mut state, brave) = brave_board(false, false);
        push_stack_spell(&mut state, P0, "Field Marshal");
        assert!(!kept(&state, cast(&state, brave)));
    }

    #[test]
    fn keeps_protection_against_an_opposing_spell_or_board() {
        let (state, brave) = brave_board(false, true);
        assert!(
            kept(&state, cast(&state, brave)),
            "opposing spell on the stack"
        );
        let (state, brave) = brave_board(true, false);
        assert!(kept(&state, cast(&state, brave)), "opposing creature");
    }

    // ── Shadow of the Second Sun on the opponent ───────────────────────────

    /// Field report (22/10): the AI enchanted the OPPONENT with Shadow of the
    /// Second Sun. Production path: cast it and read the reducer's
    /// `TargetSelection`.
    #[test]
    fn rejects_extra_phase_aura_on_the_opponent() {
        let mut scenario = GameScenario::new();
        scenario.at_phase(Phase::PreCombatMain);
        let shadow = scenario
            .add_spell_to_hand_from_oracle(P0, "Shadow of the Second Sun", false, SHADOW)
            .as_enchantment()
            .with_subtypes(vec!["Aura"])
            .with_keyword(Keyword::Enchant(TargetFilter::Player))
            .with_mana_cost(ManaCost::zero())
            .id();
        let mut runner = scenario.build();
        priority_to_p0(&mut runner, P0);
        let action = cast(runner.state(), shadow);
        runner.act(action).expect("cast Shadow of the Second Sun");
        let state = runner.state();
        assert!(
            matches!(state.waiting_for, WaitingFor::TargetSelection { .. }),
            "precondition: the Aura asks for its player, got {:?}",
            state.waiting_for
        );
        assert!(
            aura_is_boon_to_enchanted_player(&state.objects[&shadow]),
            "precondition: parsed as an extra-phase boon to the enchanted player"
        );
        let choose = |player| GameAction::ChooseTarget {
            target: Some(TargetRef::Player(player)),
        };
        assert!(!kept(state, choose(P1)));
        assert!(kept(state, choose(P0)));
    }

    // ── Bounce land returning itself ───────────────────────────────────────

    /// Field report (20/10): one Mountain on the battlefield and Izzet
    /// Boilerworks bouncing itself turn after turn.
    #[test]
    fn rejects_bounce_land_returning_itself_when_another_land_exists() {
        let mut scenario = GameScenario::new();
        scenario.at_phase(Phase::PreCombatMain);
        let mountain = scenario.add_basic_land(P0, ManaColor::Red);
        let boilerworks = scenario
            .add_land_to_hand(P0, "Izzet Boilerworks")
            .from_oracle_text(BOILERWORKS)
            .id();
        let mut runner = scenario.build();
        priority_to_p0(&mut runner, P0);
        let card_id = runner.state().objects[&boilerworks].card_id;
        runner
            .act(GameAction::PlayLand {
                object_id: boilerworks,
                card_id,
            })
            .expect("play Izzet Boilerworks");
        for _ in 0..4 {
            if matches!(
                runner.state().waiting_for,
                WaitingFor::EffectZoneChoice { .. }
            ) {
                break;
            }
            runner.pass_both_players();
        }
        let state = runner.state();
        assert!(
            matches!(state.waiting_for, WaitingFor::EffectZoneChoice { .. }),
            "precondition: the ETB asks which land returns, got {:?}",
            state.waiting_for
        );
        assert!(!kept(
            state,
            GameAction::SelectCards {
                cards: vec![boilerworks]
            }
        ));
        assert!(kept(
            state,
            GameAction::SelectCards {
                cards: vec![mountain]
            }
        ));
    }

    #[test]
    fn bounce_land_may_return_itself_when_it_is_the_only_land() {
        let mut scenario = GameScenario::new();
        scenario.at_phase(Phase::PreCombatMain);
        let boilerworks = scenario
            .add_land_to_hand(P0, "Izzet Boilerworks")
            .from_oracle_text(BOILERWORKS)
            .id();
        let mut runner = scenario.build();
        priority_to_p0(&mut runner, P0);
        let card_id = runner.state().objects[&boilerworks].card_id;
        runner
            .act(GameAction::PlayLand {
                object_id: boilerworks,
                card_id,
            })
            .expect("play Izzet Boilerworks");
        for _ in 0..4 {
            if matches!(
                runner.state().waiting_for,
                WaitingFor::EffectZoneChoice { .. }
            ) {
                break;
            }
            runner.pass_both_players();
        }
        let state = runner.state();
        if matches!(state.waiting_for, WaitingFor::EffectZoneChoice { .. }) {
            assert!(kept(
                state,
                GameAction::SelectCards {
                    cards: vec![boilerworks]
                }
            ));
        }
    }

    // ── Attacking with no power ────────────────────────────────────────────

    /// Field reports (19/10 and 22/10): Market Gnome 0/3 and a 0/1 Wizard swung
    /// into open blockers for nothing.
    #[test]
    fn zero_power_attacker_without_attack_reward_is_pointless() {
        let mut scenario = GameScenario::new();
        let gnome = scenario.add_vanilla(P0, 0, 3);
        let bear = scenario.add_vanilla(P0, 2, 2);
        let runner = scenario.build();
        let state = runner.state();
        assert!(zero_power_attack_is_pointless(state, P0, gnome));
        assert!(!zero_power_attack_is_pointless(state, P0, bear));
    }

    #[test]
    fn zero_power_attacker_with_exalted_on_board_is_kept() {
        let mut scenario = GameScenario::new();
        let gnome = scenario.add_vanilla(P0, 0, 3);
        scenario
            .add_creature(P0, "Exalted Angel", 4, 5)
            .with_keyword(Keyword::Exalted);
        let runner = scenario.build();
        assert!(!zero_power_attack_is_pointless(runner.state(), P0, gnome));
    }

    /// The Market Gnome board: no opposing blocker can kill the 0/3, so the
    /// attack looked "safe" — and dealt nothing.
    #[test]
    fn choose_attackers_leaves_a_zero_power_creature_home() {
        for blockers in [vec![], vec![(2, 4), (1, 1)]] {
            let mut scenario = GameScenario::new();
            scenario.at_phase(Phase::DeclareAttackers);
            let gnome = scenario.add_vanilla(P0, 0, 3);
            for (power, toughness) in &blockers {
                scenario.add_vanilla(P1, *power, *toughness);
            }
            let runner = scenario.build();
            let attackers = crate::combat_ai::choose_attackers(runner.state(), P0);
            assert!(
                !attackers.contains(&gnome),
                "blockers {blockers:?}: {attackers:?}"
            );
        }
    }

    // ── Harnessed Lightning: pay what kills ────────────────────────────────

    const HARNESSED: &str = "Choose target creature. You get {E}{E}{E} (three energy counters), then you may pay any amount of {E}. Harnessed Lightning deals that much damage to that creature.";

    /// Harnessed Lightning (free) in P0's hand, `energy` in P0's pool and the
    /// given `(controller, toughness)` creatures (all 3-power) on the
    /// battlefield, with P0 holding priority in the precombat main phase.
    fn harnessed_runner(
        creatures: &[(PlayerId, i32)],
        energy: u32,
    ) -> (GameRunner, ObjectId, Vec<ObjectId>) {
        let mut scenario = GameScenario::new();
        scenario.at_phase(Phase::PreCombatMain);
        let ids = creatures
            .iter()
            .map(|(controller, toughness)| scenario.add_vanilla(*controller, 3, *toughness))
            .collect();
        let lightning = scenario
            .add_spell_to_hand_from_oracle(P0, "Harnessed Lightning", true, HARNESSED)
            .with_mana_cost(ManaCost::zero())
            .id();
        let mut runner = scenario.build();
        runner.state_mut().players[0].energy = energy;
        priority_to_p0(&mut runner, P0);
        (runner, lightning, ids)
    }

    /// Casts Harnessed Lightning (free) at a 3/`toughness` controlled by
    /// `owner` with 9 energy in the pool (12 after the spell's own three) and
    /// runs to the pay prompt.
    fn harnessed_board_at(owner: PlayerId, toughness: i32) -> (GameState, ObjectId) {
        let (mut runner, lightning, ids) = harnessed_runner(&[(owner, toughness)], 9);
        let victim = ids[0];
        let action = cast(runner.state(), lightning);
        runner.act(action).expect("cast Harnessed Lightning");
        if matches!(
            runner.state().waiting_for,
            WaitingFor::TargetSelection { .. }
        ) {
            runner
                .act(GameAction::ChooseTarget {
                    target: Some(TargetRef::Object(victim)),
                })
                .expect("choose the creature");
        }
        for _ in 0..8 {
            match runner.state().waiting_for {
                WaitingFor::PayAmountChoice { .. } => break,
                // "you may pay any amount": the engine asks whether to pay at
                // all before asking how much.
                WaitingFor::OptionalEffectChoice { .. } => {
                    runner
                        .act(GameAction::DecideOptionalEffect { accept: true })
                        .expect("accept the optional payment");
                }
                _ => runner.pass_both_players(),
            }
        }
        let state = runner.state().clone();
        assert!(
            matches!(state.waiting_for, WaitingFor::PayAmountChoice { .. }),
            "precondition: the spell asks how much energy to pay, got {:?}",
            state.waiting_for
        );
        (state, victim)
    }

    fn harnessed_board(toughness: i32) -> (GameState, ObjectId) {
        harnessed_board_at(P1, toughness)
    }

    fn pay_range(state: &GameState) -> (u32, u32) {
        let WaitingFor::PayAmountChoice { min, max, .. } = &state.waiting_for else {
            unreachable!("harnessed_board_at stops at the pay prompt");
        };
        (*min, *max)
    }

    fn pay(amount: u32) -> GameAction {
        GameAction::SubmitPayAmount { amount }
    }

    /// Field report (24/10, Kaladesh Energy): 12 energy, an 18/6 as the
    /// target, paid 1. Only the lethal amount is kept.
    #[test]
    fn rejects_energy_amount_that_does_not_kill() {
        let (state, _) = harnessed_board(6);
        let (min, max) = pay_range(&state);
        assert!(min <= 6 && max >= 6, "range {min}..={max}");
        assert!(kept(&state, pay(6)), "lethal");
        assert!(!kept(&state, pay(1)), "one energy");
        assert!(!kept(&state, pay(0)), "nothing");
        assert!(!kept(&state, pay(max)), "everything");
    }

    /// A 20-toughness creature survives all 12 energy: paying any of it
    /// destroys nothing, so the energy is kept.
    #[test]
    fn keeps_the_energy_when_the_creature_cannot_die() {
        let (state, _) = harnessed_board(20);
        let (min, max) = pay_range(&state);
        assert_eq!(min, 0);
        assert!(kept(&state, pay(0)), "keep the energy");
        assert!(!kept(&state, pay(1)), "one energy");
        assert!(!kept(&state, pay(max)), "everything");
    }

    /// CR 510.1 + CR 120.6: the same creature blocking the AI's attacker takes
    /// combat damage too, so a partial payment can still finish it.
    #[test]
    fn energy_amount_is_free_when_the_creature_is_in_combat_with_ours() {
        let (mut state, victim) = harnessed_board(20);
        let card_id = CardId(state.next_object_id);
        let attacker = create_object(
            &mut state,
            card_id,
            P0,
            "Attacker".to_string(),
            Zone::Battlefield,
        );
        let mut combat = engine::game::combat::CombatState::default();
        combat.blocker_to_attacker.insert(victim, vec![attacker]);
        state.combat = Some(combat);
        let (_, max) = pay_range(&state);
        assert!(kept(&state, pay(0)));
        assert!(kept(&state, pay(max)));
    }

    /// Cast at the AI's own creature (only for its energy), nothing is paid:
    /// the damage would only hurt the AI.
    #[test]
    fn pays_nothing_into_its_own_creature() {
        let (state, _) = harnessed_board_at(P0, 2);
        assert!(kept(&state, pay(0)), "nothing");
        assert!(
            !kept(&state, pay(2)),
            "the lethal amount into its own creature"
        );
    }

    fn harnessed_targeting(
        creatures: &[(PlayerId, i32)],
        energy: u32,
    ) -> (GameState, Vec<ObjectId>) {
        let (mut runner, lightning, ids) = harnessed_runner(creatures, energy);
        let action = cast(runner.state(), lightning);
        runner.act(action).expect("cast Harnessed Lightning");
        let state = runner.state().clone();
        assert!(
            matches!(state.waiting_for, WaitingFor::TargetSelection { .. }),
            "precondition: two or more legal targets, got {:?}",
            state.waiting_for
        );
        (state, ids)
    }

    fn aim(id: ObjectId) -> GameAction {
        GameAction::ChooseTarget {
            target: Some(TargetRef::Object(id)),
        }
    }

    /// CR 601.2c: with no energy in the pool the budget is the spell's own
    /// three — a 3/2 dies, a 3/20 doesn't. The unkillable target is rejected
    /// while the killable one is legal.
    #[test]
    fn aims_at_the_creature_it_can_kill() {
        let (state, ids) = harnessed_targeting(&[(P1, 2), (P1, 20)], 0);
        assert!(kept(&state, aim(ids[0])), "the 3/2 dies to three energy");
        assert!(!kept(&state, aim(ids[1])), "the 3/20 survives");
    }

    /// The spell's own three energy counts before the payment: with 3 in the
    /// pool a 3/6 dies, so it stays a legal choice next to a 3/2.
    #[test]
    fn the_spells_own_energy_counts_toward_the_kill() {
        let (state, ids) = harnessed_targeting(&[(P1, 2), (P1, 6)], 3);
        assert!(kept(&state, aim(ids[0])));
        assert!(kept(&state, aim(ids[1])));
    }

    /// Nothing dies: no target is preferred over another (the cast itself is
    /// what the gate rejects).
    #[test]
    fn no_target_is_rejected_when_none_can_die() {
        let (state, ids) = harnessed_targeting(&[(P1, 20), (P1, 30)], 0);
        assert!(kept(&state, aim(ids[0])));
        assert!(kept(&state, aim(ids[1])));
    }

    #[test]
    fn own_creature_is_not_aimed_at_while_an_opposing_one_dies() {
        let (state, ids) = harnessed_targeting(&[(P0, 2), (P1, 2)], 0);
        assert!(!kept(&state, aim(ids[0])), "own 3/2");
        assert!(kept(&state, aim(ids[1])), "opposing 3/2");
    }

    fn harnessed_cast_kept(creatures: &[(PlayerId, i32)], energy: u32) -> bool {
        let (runner, lightning, _) = harnessed_runner(creatures, energy);
        kept(runner.state(), cast(runner.state(), lightning))
    }

    /// The card is removal: cast only when some opposing creature dies to the
    /// pool plus the spell's own three energy.
    #[test]
    fn casts_only_with_an_opposing_creature_to_kill() {
        assert!(harnessed_cast_kept(&[(P1, 3)], 0), "a 3/3 dies to three");
        assert!(!harnessed_cast_kept(&[(P1, 20)], 0), "a 3/20 doesn't");
        assert!(
            harnessed_cast_kept(&[(P1, 20)], 17),
            "17 + 3 kills the 3/20"
        );
        assert!(
            !harnessed_cast_kept(&[(P0, 2)], 0),
            "only its own creature: the card would buy three energy and nothing else"
        );
    }

    // ── Rancor with no own creature ────────────────────────────────────────

    const RANCOR: &str = "Enchant creature\nEnchanted creature gets +2/+0 and has trample.\nWhen Rancor is put into a graveyard from the battlefield, return Rancor to its owner's hand.";

    fn rancor_runner(own_creature: bool) -> (GameRunner, ObjectId, ObjectId, Option<ObjectId>) {
        let mut scenario = GameScenario::new();
        scenario.at_phase(Phase::PreCombatMain);
        let foe = scenario.add_vanilla(P1, 2, 2);
        let own = own_creature.then(|| scenario.add_vanilla(P0, 2, 2));
        let rancor = scenario
            .add_spell_to_hand_from_oracle(P0, "Rancor", false, RANCOR)
            .as_enchantment()
            .with_subtypes(vec!["Aura"])
            .with_keyword(Keyword::Enchant(TargetFilter::Typed(
                TypedFilter::creature(),
            )))
            .with_mana_cost(ManaCost::zero())
            .id();
        let mut runner = scenario.build();
        priority_to_p0(&mut runner, P0);
        let parsed = &runner.state().objects[&rancor];
        assert!(
            aura_is_boon_to_enchanted_creature(parsed),
            "precondition: Rancor parsed as a boon-only creature Aura — subtypes {:?}, keywords {:?}, statics {:?}",
            parsed.card_types.subtypes,
            parsed.keywords,
            parsed.static_definitions.iter_unchecked().collect::<Vec<_>>()
        );
        (runner, rancor, foe, own)
    }

    /// Field report (23/10): the AI had no creature and cast Rancor — its only
    /// legal target was the opponent's.
    #[test]
    fn rejects_boon_aura_cast_with_only_foreign_creatures() {
        let (runner, rancor, _, _) = rancor_runner(false);
        let state = runner.state();
        assert!(!kept(state, cast(state, rancor)));
    }

    #[test]
    fn boon_aura_with_an_own_creature_is_cast_and_aimed_at_it() {
        let (mut runner, rancor, foe, own) = rancor_runner(true);
        let own = own.expect("own creature");
        let action = cast(runner.state(), rancor);
        assert!(kept(runner.state(), action.clone()));
        runner.act(action).expect("cast Rancor");
        let state = runner.state();
        assert!(
            matches!(state.waiting_for, WaitingFor::TargetSelection { .. }),
            "precondition: the Aura asks for its creature, got {:?}",
            state.waiting_for
        );
        let choose = |id| GameAction::ChooseTarget {
            target: Some(TargetRef::Object(id)),
        };
        assert!(!kept(state, choose(foe)), "the opponent's creature");
        assert!(kept(state, choose(own)), "the AI's own creature");
    }

    // ── Hanweir Battlements: haste for a creature that can already attack ──

    const BATTLEMENTS: &str = "{R}, {T}: Target creature gains haste until end of turn.";

    fn battlements_runner(
        phase: Phase,
        sick_creature: bool,
    ) -> (GameRunner, ObjectId, ObjectId, Option<ObjectId>, usize) {
        let mut scenario = GameScenario::new();
        scenario.at_phase(phase);
        scenario.add_basic_land(P0, ManaColor::Red);
        scenario.add_basic_land(P0, ManaColor::Red);
        let veteran = scenario.add_vanilla(P0, 3, 3);
        let fresh = sick_creature.then(|| {
            scenario
                .add_creature(P0, "Fresh Bear", 2, 2)
                .with_summoning_sickness()
                .id()
        });
        let land = scenario
            .add_land_from_oracle(P0, "Hanweir Battlements", BATTLEMENTS)
            .id();
        let mut runner = scenario.build();
        priority_to_p0(&mut runner, P0);
        let index = activated_ability_definitions(runner.state(), land)
            .into_iter()
            .find(|(_, definition)| effect_is_haste_grant(&definition.effect))
            .map(|(index, _)| index)
            .expect("precondition: the land parsed a haste grant");
        (runner, land, veteran, fresh, index)
    }

    /// Field report (23/10): {R},{T} every upkeep to hasten a creature that had
    /// been on the battlefield for turns.
    #[test]
    fn rejects_haste_grant_when_no_creature_needs_haste() {
        let (runner, land, _, _, index) = battlements_runner(Phase::PreCombatMain, false);
        assert!(!kept(
            runner.state(),
            GameAction::ActivateAbility {
                payment_mode: engine::types::game_state::CastPaymentMode::Auto,
                source_id: land,
                ability_index: index
            }
        ));
    }

    #[test]
    fn rejects_haste_grant_after_combat_even_with_a_fresh_creature() {
        let (runner, land, _, _, index) = battlements_runner(Phase::PostCombatMain, true);
        assert!(!kept(
            runner.state(),
            GameAction::ActivateAbility {
                payment_mode: engine::types::game_state::CastPaymentMode::Auto,
                source_id: land,
                ability_index: index
            }
        ));
    }

    #[test]
    fn haste_grant_with_a_fresh_creature_is_kept_and_aimed_at_it() {
        let (mut runner, land, veteran, fresh, index) =
            battlements_runner(Phase::PreCombatMain, true);
        let fresh = fresh.expect("fresh creature");
        let activate = GameAction::ActivateAbility {
            payment_mode: engine::types::game_state::CastPaymentMode::Auto,
            source_id: land,
            ability_index: index,
        };
        assert!(kept(runner.state(), activate.clone()));
        runner.act(activate).expect("activate Hanweir Battlements");
        let state = runner.state();
        if matches!(state.waiting_for, WaitingFor::TargetSelection { .. }) {
            let choose = |id| GameAction::ChooseTarget {
                target: Some(TargetRef::Object(id)),
            };
            assert!(
                !kept(state, choose(veteran)),
                "the creature that can already attack"
            );
            assert!(kept(state, choose(fresh)), "the summoning-sick creature");
        }
    }
}
