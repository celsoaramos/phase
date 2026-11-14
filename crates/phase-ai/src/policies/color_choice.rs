//! Colour named-choice policy, and the "colorless or a colour" branch.
//!
//! Report (owner, 2026-10-30, Giver of Runes): "the engine picks the colour and
//! the card name without criteria — the robot casts, but decides by a simple
//! heuristic". Reproduced in `tests/ai_quality.rs`: with an opponent's red
//! burn spell aimed at the creature Giver protects, the robot took the colour
//! branch and named "White" — the first option of the WUBRG list — and did the
//! same against a colourless spell. No policy scored `ChooseOption` for a
//! `ChoiceType::Color` prompt, nor the `ChooseBranch` of the "protection from
//! colorless or from the color of your choice" mode, so candidate order won.
//!
//! This policy reads WHAT the choice feeds before scoring it: the resolving
//! ability (the paused continuation of a `NamedChoice`, or the branches of a
//! `ChooseOneOfBranch`) must grant protection or hexproof from the chosen
//! colour (or protection from colorless) to objects the AI controls. Then each
//! option is scored by what that grant would actually stop, asked of the
//! engine's own protection test (`source_matches_protection_target`):
//!
//! 1. every opponent stack entry with a harmful node acting on a protected
//!    object, whose source the grant would match (CR 702.16b / CR 702.11d:
//!    the spell or ability loses its target and does nothing to it);
//! 2. every opponent creature on the battlefield the grant would match
//!    (CR 702.16c-e: it can't block the protected creature, and its damage is
//!    prevented), capped — a tiebreak when nothing is on the stack.
//!
//! Hexproof only stops targeting, so it scores the stack and not the board.
//!
//! Limitations: a colour choice whose purpose is anything other than a
//! defensive grant (an anthem, a mana colour, a "destroy all of the chosen
//! color" sweep) is left neutral — the purpose is not guessed. An as-enters
//! choice (Floating Shield) has no paused continuation and is not read either.

use std::str::FromStr;

use engine::game::effects::stack_reach::stack_entry_node_reach;
use engine::game::keywords::source_matches_protection_target;
use engine::types::ability::{
    AbilityDefinition, ChoiceType, ContinuousModification, Effect, ResolvedAbility, TargetRef,
};
use engine::types::actions::GameAction;
use engine::types::card_type::CoreType;
use engine::types::game_state::{GameState, WaitingFor};
use engine::types::identifiers::ObjectId;
use engine::types::keywords::{HexproofFilter, Keyword, ProtectionTarget};
use engine::types::mana::ManaColor;
use engine::types::player::PlayerId;

use super::context::PolicyContext;
use super::effect_classify::{effect_polarity, EffectPolarity};
use super::registry::{DecisionKind, PolicyId, PolicyReason, PolicyVerdict, TacticalPolicy};
use crate::features::DeckFeatures;

/// Opponent creatures past the fourth add no further discrimination between
/// colours; the uncapped count would let a wide board outvote a spell that is
/// about to kill the protected creature.
const BOARD_CAP: usize = 4;

/// What a granted keyword shields from, read off the ability's definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shield {
    /// CR 702.16: protection from the chosen colour.
    ProtectionChosenColor,
    /// CR 702.11d: hexproof from the chosen colour — targeting only.
    HexproofChosenColor,
    /// CR 702.16 + CR 105.2c: protection from colorless.
    ProtectionColorless,
}

impl Shield {
    /// The concrete keyword target this shield becomes once `color` is chosen
    /// (`None` for the colorless shield, which needs no choice).
    fn protection_target(self, color: Option<ManaColor>) -> Option<ProtectionTarget> {
        match (self, color) {
            (Shield::ProtectionColorless, _) => {
                Some(ProtectionTarget::Quality("colorless".to_string()))
            }
            (Shield::ProtectionChosenColor | Shield::HexproofChosenColor, Some(color)) => {
                Some(ProtectionTarget::Color(color))
            }
            (_, None) => None,
        }
    }

    fn stops_blockers_and_damage(self) -> bool {
        !matches!(self, Shield::HexproofChosenColor)
    }

    fn needs_color(self) -> bool {
        !matches!(self, Shield::ProtectionColorless)
    }
}

fn effect_shields(effect: &Effect, out: &mut Vec<Shield>) {
    let Effect::GenericEffect {
        static_abilities, ..
    } = effect
    else {
        return;
    };
    for definition in static_abilities {
        for modification in &definition.modifications {
            let ContinuousModification::AddKeyword { keyword } = modification else {
                continue;
            };
            match keyword {
                Keyword::Protection(ProtectionTarget::ChosenColor) => {
                    out.push(Shield::ProtectionChosenColor)
                }
                Keyword::HexproofFrom(HexproofFilter::ChosenColor) => {
                    out.push(Shield::HexproofChosenColor)
                }
                Keyword::Protection(ProtectionTarget::Quality(quality))
                    if quality.eq_ignore_ascii_case("colorless") =>
                {
                    out.push(Shield::ProtectionColorless)
                }
                _ => {}
            }
        }
    }
}

fn resolved_shields(ability: &ResolvedAbility, out: &mut Vec<Shield>) {
    effect_shields(&ability.effect, out);
    if let Some(sub) = &ability.sub_ability {
        resolved_shields(sub, out);
    }
    if let Some(other) = &ability.else_ability {
        resolved_shields(other, out);
    }
}

fn definition_shields(definition: &AbilityDefinition, out: &mut Vec<Shield>) {
    effect_shields(&definition.effect, out);
    if let Some(sub) = &definition.sub_ability {
        definition_shields(sub, out);
    }
    if let Some(other) = &definition.else_ability {
        definition_shields(other, out);
    }
}

fn resolved_object_targets(ability: &ResolvedAbility, out: &mut Vec<ObjectId>) {
    for target in &ability.targets {
        if let TargetRef::Object(id) = target {
            if !out.contains(id) {
                out.push(*id);
            }
        }
    }
    if let Some(sub) = &ability.sub_ability {
        resolved_object_targets(sub, out);
    }
    if let Some(other) = &ability.else_ability {
        resolved_object_targets(other, out);
    }
}

/// The AI-controlled battlefield objects the grant lands on: the ability's
/// object targets, or — for "this creature gains …" (Knight of Dawn), which
/// targets nothing — its source.
fn protected_objects(
    state: &GameState,
    ai: PlayerId,
    targets: Vec<ObjectId>,
    source: ObjectId,
) -> Vec<ObjectId> {
    let candidates = if targets.is_empty() {
        vec![source]
    } else {
        targets
    };
    candidates
        .into_iter()
        .filter(|id| {
            state
                .objects
                .get(id)
                .is_some_and(|obj| obj.controller == ai && state.battlefield.contains(id))
        })
        .collect()
}

/// What the shield would stop for these protected objects, split into the
/// opponent stack entries it fizzles and the opponent creatures it walls off.
fn threats_stopped(
    state: &GameState,
    ai: PlayerId,
    protected: &[ObjectId],
    shield: Shield,
    protection: &ProtectionTarget,
) -> (usize, usize) {
    let protected_objs: Vec<_> = protected
        .iter()
        .filter_map(|id| state.objects.get(id))
        .collect();
    if protected_objs.is_empty() {
        return (0, 0);
    }

    let stack = state
        .stack
        .iter()
        .filter(|entry| entry.controller != ai)
        .filter(|entry| {
            let Some(source) = state.objects.get(&entry.source_id) else {
                return false;
            };
            stack_entry_node_reach(state, entry).iter().any(|reach| {
                matches!(effect_polarity(&reach.node.effect), EffectPolarity::Harmful)
                    && protected_objs.iter().any(|obj| {
                        reach.acted_on.contains(&TargetRef::Object(obj.id))
                            && source_matches_protection_target(protection, obj, source)
                    })
            })
        })
        .count();

    let board = if shield.stops_blockers_and_damage() {
        state
            .battlefield
            .iter()
            .filter_map(|id| state.objects.get(id))
            .filter(|obj| {
                obj.controller != ai && obj.card_types.core_types.contains(&CoreType::Creature)
            })
            .filter(|creature| {
                protected_objs
                    .iter()
                    .any(|obj| source_matches_protection_target(protection, obj, creature))
            })
            .count()
            .min(BOARD_CAP)
    } else {
        0
    };
    (stack, board)
}

/// Best value of one shield: for a colour shield, the best of the colours the
/// prompt allows; for the colorless shield, itself.
fn shield_value(
    ctx: &PolicyContext<'_>,
    protected: &[ObjectId],
    shield: Shield,
    color: Option<ManaColor>,
    excluded: &[ManaColor],
) -> (f64, usize, usize) {
    let penalties = &ctx.config.policy_penalties;
    let score = |color: Option<ManaColor>| {
        let Some(protection) = shield.protection_target(color) else {
            return (0.0, 0, 0);
        };
        let (stack, board) =
            threats_stopped(ctx.state, ctx.ai_player, protected, shield, &protection);
        (
            stack as f64 * penalties.chosen_color_stack_threat
                + board as f64 * penalties.chosen_color_board_unit,
            stack,
            board,
        )
    };
    match (shield.needs_color(), color) {
        (false, _) | (true, Some(_)) => score(color),
        (true, None) => ManaColor::ALL
            .into_iter()
            .filter(|c| !excluded.contains(c))
            .map(|c| score(Some(c)))
            .fold(
                (0.0, 0, 0),
                |best, next| if next.0 > best.0 { next } else { best },
            ),
    }
}

pub struct ColorChoicePolicy;

impl TacticalPolicy for ColorChoicePolicy {
    fn id(&self) -> PolicyId {
        PolicyId::ColorChoice
    }

    fn decision_kinds(&self) -> &'static [DecisionKind] {
        // `decision_kind::classify` routes `NamedChoice` and `ChooseOneOfBranch`
        // into the `ActivateAbility` bucket.
        &[DecisionKind::ActivateAbility]
    }

    fn activation(
        &self,
        _features: &DeckFeatures,
        state: &GameState,
        _player: PlayerId,
    ) -> Option<f32> {
        match state.waiting_for {
            WaitingFor::NamedChoice {
                choice_type: ChoiceType::Color { .. },
                ..
            }
            | WaitingFor::ChooseOneOfBranch { .. } => {
                // The verdict decides whether the prompt feeds a protection grant at all.
                // activation-constant: prompt-shape gate, no deck scaling.
                Some(1.0)
            }
            _ => None,
        }
    }

    fn verdict(&self, ctx: &PolicyContext<'_>) -> PolicyVerdict {
        let na = || PolicyVerdict::neutral(PolicyReason::new("color_choice_na"));

        let (value, stack, board) = match &ctx.decision.waiting_for {
            WaitingFor::NamedChoice {
                player,
                choice_type: ChoiceType::Color { excluded },
                ..
            } => {
                if *player != ctx.ai_player {
                    return na();
                }
                let GameAction::ChooseOption { choice } = &ctx.candidate.action else {
                    return na();
                };
                let Ok(color) = ManaColor::from_str(choice) else {
                    return na();
                };
                // CR 608.2c: the rest of the resolving ability is parked while
                // the choice is open — that is what the colour feeds.
                let Some(frame) = ctx.state.resolution_stack.active_ability_continuation() else {
                    return na();
                };
                let chain = &frame.pending.chain;
                let mut shields = Vec::new();
                resolved_shields(chain, &mut shields);
                let Some(shield) = shields.into_iter().find(|s| s.needs_color()) else {
                    return na();
                };
                let mut targets = Vec::new();
                resolved_object_targets(chain, &mut targets);
                let protected =
                    protected_objects(ctx.state, ctx.ai_player, targets, chain.source_id);
                shield_value(ctx, &protected, shield, Some(color), excluded)
            }
            WaitingFor::ChooseOneOfBranch {
                player,
                source_id,
                branches,
                parent_occurrences,
                ..
            } => {
                if *player != ctx.ai_player {
                    return na();
                }
                let GameAction::ChooseBranch { index } = &ctx.candidate.action else {
                    return na();
                };
                // Only a branch set where some branch grants a colour shield is
                // this policy's business; every other "choose one" stays
                // neutral.
                let branch_shields: Vec<Vec<Shield>> = branches
                    .iter()
                    .map(|branch| {
                        let mut out = Vec::new();
                        definition_shields(branch, &mut out);
                        out
                    })
                    .collect();
                if branch_shields.iter().all(Vec::is_empty) {
                    return na();
                }
                let Some(shield) = branch_shields.get(*index).and_then(|s| s.first().copied())
                else {
                    return na();
                };
                let targets = parent_occurrences
                    .targets
                    .iter()
                    .filter_map(|t| match t {
                        TargetRef::Object(id) => Some(*id),
                        _ => None,
                    })
                    .collect();
                let protected = protected_objects(ctx.state, ctx.ai_player, targets, *source_id);
                shield_value(ctx, &protected, shield, None, &[])
            }
            _ => return na(),
        };

        if value == 0.0 {
            return PolicyVerdict::neutral(PolicyReason::new("color_choice_no_threat"));
        }
        PolicyVerdict::score(
            value,
            PolicyReason::new("color_choice")
                .with_fact("stack_threats", stack as i64)
                .with_fact("board_creatures", board as i64),
        )
    }
}
