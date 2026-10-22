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

use engine::game::game_object::GameObject;
use engine::game::players::is_opponent;
use engine::game::targeting::find_legal_targets;
use engine::types::ability::{
    AbilityCost, AbilityDefinition, AbilityKind, ContinuousModification, ControllerRef, Duration,
    Effect, EffectKind, ExtraPhaseRecipient, TargetFilter, TargetRef, UnlessPayModifier,
};
use engine::types::actions::GameAction;
use engine::types::card_type::CoreType;
use engine::types::game_state::{GameState, StackEntryKind, WaitingFor};
use engine::types::identifiers::ObjectId;
use engine::types::keywords::Keyword;
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
        }
        GameAction::ChooseTarget {
            target: Some(target),
        } => target_is_rejected(ctx, target),
        GameAction::SelectTargets { targets } => {
            targets.iter().any(|target| target_is_rejected(ctx, target))
        }
        GameAction::SelectCards { cards } => bounce_land_returns_itself(ctx, cards),
        _ => false,
    }
}

fn target_is_rejected(ctx: &PolicyContext<'_>, target: &TargetRef) -> bool {
    soft_counter_target_is_payable(ctx, target) || boon_aura_targets_opponent(ctx, target)
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
}
