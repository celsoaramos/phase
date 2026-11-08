use std::fmt;

use serde::{Deserialize, Serialize};

/// CR 122.1b: Named player counter types tracked by the engine.
/// Poison counters route to the dedicated `poison_counters` field due to SBA rules (CR 704.5c).
/// Energy counters are excluded — they use the dedicated `energy` field and `GainEnergy` effect.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum PlayerCounterKind {
    Poison,
    Experience,
    Rad,
    Ticket,
}

impl fmt::Display for PlayerCounterKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Poison => write!(f, "poison"),
            Self::Experience => write!(f, "experience"),
            Self::Rad => write!(f, "rad"),
            Self::Ticket => write!(f, "ticket"),
        }
    }
}

/// CR 122.1: The kind of a counter a player can have. Named kinds live in
/// `PlayerCounterKind`; energy is a player counter too, but the engine stores
/// it apart (`Player::energy`), so a player's counter kind is one of the two.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PlayerCounterType {
    Kind(PlayerCounterKind),
    Energy,
}

impl PlayerCounterType {
    /// Every player counter kind, in a stable order.
    pub const ALL: [PlayerCounterType; 5] = [
        PlayerCounterType::Kind(PlayerCounterKind::Poison),
        PlayerCounterType::Kind(PlayerCounterKind::Experience),
        PlayerCounterType::Kind(PlayerCounterKind::Rad),
        PlayerCounterType::Kind(PlayerCounterKind::Ticket),
        PlayerCounterType::Energy,
    ];
}

impl fmt::Display for PlayerCounterType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Kind(kind) => kind.fmt(f),
            Self::Energy => write!(f, "energy"),
        }
    }
}

impl std::str::FromStr for PlayerCounterType {
    type Err = ();

    /// Inverse of `Display`.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|kind| kind.to_string() == s)
            .ok_or(())
    }
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct PlayerId(pub u8);
