//! CR 608.2c: which quantities and player filters read a chain's tracked
//! object set. Pure readers of the ability AST, so they live in the types layer:
//! the effect resolver uses them as its publication gate and the parser uses
//! the same authority to mark a delayed trigger that consumes the set.

use crate::types::ability::{CardTypeSetSource, PlayerFilter, QuantityExpr, QuantityRef};

/// CR 608.2c: Does a [`CardTypeSetSource`] population read the chain's tracked
/// object set, at any depth of its `AnyOf` union?
///
/// The single authority for the tracked-set dependency on the population axis.
/// A direct-only `TrackedSet { .. }` match misses a tracked set nested inside an
/// `AnyOf` ("among cards exiled with ~ and creatures you control"), so the
/// producer never publishes and the chained count resolves to 0. Uses the same
/// bounded walker as every other union consumer; an incomplete walk is
/// conservatively reported as a dependency so a truncated source still forces
/// publication rather than silently under-counting.
fn card_type_set_source_references_tracked_set(source: &CardTypeSetSource) -> bool {
    let mut found = false;
    let complete =
        source.try_for_each_member(crate::types::ability::UNION_DEPTH_BUDGET, &mut |leaf| {
            found |= matches!(leaf, CardTypeSetSource::TrackedSet { .. });
        });
    found || !complete
}

pub(crate) fn quantity_expr_references_tracked_set(qty: &QuantityExpr) -> bool {
    match qty {
        QuantityExpr::Fixed { .. } => false,
        QuantityExpr::Ref { qty } => match qty {
            QuantityRef::TrackedSetSize | QuantityRef::FilteredTrackedSetSize { .. } => true,
            // CR 608.2c: the three characteristic-set quantities share the
            // population axis, so a tracked set nested inside an `AnyOf` union
            // must be detected too — a direct-only `TrackedSet` match would let
            // the producer skip publication and the chained count resolve to 0.
            QuantityRef::DistinctCardTypes { source }
            | QuantityRef::SharedCardTypes { source }
            | QuantityRef::DistinctSubtypes { source, .. }
            | QuantityRef::DistinctColorsAmong { source } => {
                card_type_set_source_references_tracked_set(source)
            }
            QuantityRef::PropertyAggregate(aggregate) => {
                card_type_set_source_references_tracked_set(aggregate.source())
            }
            // CR 608.2c: a player-count whose filter is keyed on the chain's
            // tracked object set is a CONSUMER of that set — the preceding
            // producer must publish it, or the count resolves to 0. This is the
            // Seasoned Pyromancer (#740) shape one layer up: the tracked-set
            // reference is nested inside the PLAYER filter, not the quantity.
            // Not every `PlayerCount` qualifies, so it must be asked per filter.
            QuantityRef::PlayerCount { filter } => player_filter_references_tracked_set(filter),
            _ => false,
        },
        QuantityExpr::Offset { inner, .. }
        | QuantityExpr::ClampMin { inner, .. }
        | QuantityExpr::Multiply { inner, .. }
        | QuantityExpr::DivideRounded { inner, .. } => quantity_expr_references_tracked_set(inner),
        QuantityExpr::Sum { exprs } | QuantityExpr::Max { exprs } => {
            exprs.iter().any(quantity_expr_references_tracked_set)
        }
        QuantityExpr::UpTo { max } => quantity_expr_references_tracked_set(max),
        QuantityExpr::Power { exponent, .. } => quantity_expr_references_tracked_set(exponent),
        QuantityExpr::Difference { left, right } => {
            quantity_expr_references_tracked_set(left)
                || quantity_expr_references_tracked_set(right)
        }
    }
}

/// CR 608.2c: Does this player filter read the chain's tracked object set?
///
/// EXHAUSTIVE BY DESIGN — no `_` arm, and it must stay that way. Its sibling
/// predicates (`quantity_expr_references_tracked_set`,
/// `filter_references_tracked_set`) are `matches!`/wildcard allowlists that a
/// new variant joins silently and WRONGLY: a non-listed consumer compiles
/// clean, its producer never publishes, and the quantity resolves to 0 instead
/// of its real value. This one makes the compiler demand an answer. Grouped `|`
/// arms keep it readable; adding a variant to the `false` group is a decision,
/// not an accident.
pub(crate) fn player_filter_references_tracked_set(filter: &PlayerFilter) -> bool {
    match filter {
        // Reads `tracked_object_sets` + `tracked_set_member_causes`, which are
        // published only when `next_sub_needs_tracked_set` reports a consumer.
        PlayerFilter::TrackedSetPossessor { .. } => true,
        // Reads `last_zone_changed_ids` — a DIFFERENT ledger, unconditionally
        // recomputed after every effect and needing no publication gate.
        PlayerFilter::ZoneChangedThisWay
        // Reads the CR 701.x `player_actions_this_way` ledger.
        | PlayerFilter::PerformedActionThisWay { .. }
        // Plain relations, turn/combat ledgers, event-context anchors, vote
        // ballots, linked-exile piles and per-candidate board/scalar
        // comparisons — none consults `tracked_object_sets`.
        | PlayerFilter::Controller
        | PlayerFilter::Opponent
        | PlayerFilter::DefendingPlayer
        | PlayerFilter::OpponentLostLife
        | PlayerFilter::OpponentGainedLife
        | PlayerFilter::HasLostTheGame
        | PlayerFilter::OpponentDealtDamage { .. }
        | PlayerFilter::OpponentAttacked { .. }
        | PlayerFilter::OpponentAttackingEnchantedPlayer
        | PlayerFilter::All
        | PlayerFilter::HighestSpeed
        | PlayerFilter::OwnersOfCardsExiledBySource
        | PlayerFilter::TriggeringPlayer
        | PlayerFilter::OpponentOtherThanTriggering
        | PlayerFilter::OpponentOfTriggeringPlayer
        | PlayerFilter::OpponentOfTriggeringPlayerNotAttacked
        | PlayerFilter::VotedFor { .. }
        | PlayerFilter::ParentObjectTargetController
        | PlayerFilter::ParentObjectTargetOwner
        | PlayerFilter::GrantingObjectCaster
        | PlayerFilter::TargetPlayer
        | PlayerFilter::ChosenPlayer { .. }
        | PlayerFilter::ControlsCount { .. }
        | PlayerFilter::PlayerAttribute { .. } => false,
        // The negation wrapper inherits its inner filter's consumption: an
        // "all except <tracked-set possessor>" scope still needs the set.
        PlayerFilter::AllExcept { exclude } => player_filter_references_tracked_set(exclude),
        // The union needs the set when any member does.
        PlayerFilter::AnyOf { filters } => filters.iter().any(player_filter_references_tracked_set),
    }
}
