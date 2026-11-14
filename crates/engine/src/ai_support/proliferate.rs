//! CR 701.34a: which proliferate choices help the player who proliferates.
//!
//! Proliferate adds one more of each kind of counter already on each chosen
//! permanent or player. Whether that helps the chooser depends on whose
//! permanent/player it is and on the counter kind: a +1/+1 counter helps its
//! controller, a -1/-1 or stun counter hurts it; energy and experience help a
//! player, poison and rad hurt. So "help me" is: my things with counters good
//! for their owner, and the opponents' things with counters bad for theirs.
//!
//! Report (MagicFinder 05/11, Metastatic Evangel): the AI proliferated with its
//! opponent's energy as the only eligible choice and took it — the board
//! evaluation does not value energy, so "all" and "none" tied and "all" won by
//! candidate order. Unknown counter kinds default to "good for their owner",
//! the common case (+1/+1, loyalty, charge, lore).

use crate::types::ability::TargetRef;
use crate::types::counter::{positive_counter_entries, CounterType};
use crate::types::game_state::GameState;
use crate::types::player::{PlayerCounterKind, PlayerId};

/// Counter kinds that are a burden to the permanent's controller.
fn harms_owner(counter: &CounterType) -> bool {
    match counter {
        CounterType::Minus1Minus1 | CounterType::Stun | CounterType::Age => true,
        CounterType::PowerToughness { power, toughness } => power + toughness < 0,
        CounterType::Generic(name) => matches!(
            name.to_ascii_lowercase().as_str(),
            "doom" | "bounty" | "depletion" | "blight" | "decayed" | "void" | "rust"
        ),
        _ => false,
    }
}

/// Counter kinds whose effect on the owner depends on the card (suspend vs.
/// vanishing time counters, a battle's defense). Left out of the verdict.
fn ambiguous(counter: &CounterType) -> bool {
    matches!(counter, CounterType::Time | CounterType::Defense)
}

/// `Some(true)` when proliferating `target` helps `chooser`, `Some(false)` when
/// it hurts, `None` when the counters on it give no clear answer.
pub fn proliferate_target_helps(
    state: &GameState,
    chooser: PlayerId,
    target: &TargetRef,
) -> Option<bool> {
    let (owner, good, bad) = match target {
        TargetRef::Object(id) => {
            let object = state.objects.get(id)?;
            let (mut good, mut bad) = (0u32, 0u32);
            for (counter, _) in positive_counter_entries(&object.counters) {
                if ambiguous(counter) {
                    continue;
                }
                if harms_owner(counter) {
                    bad += 1;
                } else {
                    good += 1;
                }
            }
            (object.controller, good, bad)
        }
        TargetRef::Player(id) => {
            let player = state.players.iter().find(|p| p.id == *id)?;
            let bad = [PlayerCounterKind::Poison, PlayerCounterKind::Rad]
                .iter()
                .filter(|kind| player.player_counter(kind) > 0)
                .count() as u32;
            let good = [PlayerCounterKind::Experience, PlayerCounterKind::Ticket]
                .iter()
                .filter(|kind| player.player_counter(kind) > 0)
                .count() as u32
                + u32::from(player.energy > 0);
            (*id, good, bad)
        }
    };
    if good == bad {
        return None;
    }
    let good_for_owner = good > bad;
    Some((owner == chooser) == good_for_owner)
}

/// The eligible choices that help `chooser`, in eligibility order.
pub fn proliferate_helpful_subset(
    state: &GameState,
    chooser: PlayerId,
    eligible: &[TargetRef],
) -> Vec<TargetRef> {
    eligible
        .iter()
        .filter(|target| proliferate_target_helps(state, chooser, target) == Some(true))
        .cloned()
        .collect()
}
