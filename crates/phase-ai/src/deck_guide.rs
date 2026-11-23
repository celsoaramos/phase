//! Deck piloting guides (MagicFinder lab, 2026-11-22, Etapa 4).
//!
//! The challengers the MagicFinder table serves are a fixed pool, so each one can
//! carry a short hand-written guide: what wins the game (when the evaluation
//! cannot see it — milling), what an opening hand needs, which cards never go
//! to the bottom, and what a search should fetch first. A guide applies to the
//! deck holding EVERY card of its `signature`; nothing is keyed by a deck name,
//! so the same list in the browser finds its guide from the cards alone.
//!
//! Every consumer is gated by `AiConfig::deck_guides` (lab key `guide=1`).

use std::collections::HashSet;
use std::sync::{Arc, OnceLock};

use engine::game::deck_loading::DeckEntry;
use engine::types::card_type::CoreType;
use engine::types::game_state::GameState;
use engine::types::identifiers::ObjectId;
use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WinPlan {
    /// Empty the opponent's library (CR 104.3c).
    Mill,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MulliganGuide {
    /// Inclusive range of lands a seven-card hand may hold.
    pub lands: [u8; 2],
    /// The seven-card hand needs at least one card of EACH group.
    #[serde(default)]
    pub needs_any: Vec<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DeckGuide {
    pub id: String,
    pub signature: Vec<String>,
    #[serde(default)]
    pub win: Option<WinPlan>,
    #[serde(default)]
    pub mulligan: Option<MulliganGuide>,
    #[serde(default)]
    pub key: Vec<String>,
    #[serde(default)]
    pub tutor: Vec<String>,
}

#[derive(Deserialize)]
struct GuideFile {
    guides: Vec<DeckGuide>,
}

fn registry() -> &'static [Arc<DeckGuide>] {
    static GUIDES: OnceLock<Vec<Arc<DeckGuide>>> = OnceLock::new();
    GUIDES.get_or_init(|| {
        let file: GuideFile = serde_json::from_str(include_str!("../data/deck_guides.json"))
            .expect("deck_guides.json parses");
        file.guides.into_iter().map(Arc::new).collect()
    })
}

/// The guide whose whole signature is in `deck`, if any.
pub fn guide_for_deck(deck: &[DeckEntry]) -> Option<Arc<DeckGuide>> {
    let names: HashSet<&str> = deck.iter().map(|entry| entry.card.name.as_str()).collect();
    registry()
        .iter()
        .find(|guide| {
            !guide.signature.is_empty()
                && guide
                    .signature
                    .iter()
                    .all(|card| names.contains(card.as_str()))
        })
        .cloned()
}

/// Extra evaluation per point of `library_pressure` weight — see
/// `eval::library_pressure`.
pub const MILL_WEIGHT: f64 = 1.0;

/// Search bonus for the guide's first tutor target; each later one is worth a
/// step less. Sized above `tutor_object_score`'s 0.8 cap so the guide decides.
const TUTOR_FIRST_BONUS: f64 = 2.0;
const TUTOR_STEP: f64 = 0.1;

impl DeckGuide {
    pub fn is_key(&self, name: &str) -> bool {
        self.key.iter().any(|card| card == name)
    }

    /// The bonus a search adds for fetching `name`.
    pub fn tutor_bonus(&self, name: &str) -> f64 {
        self.tutor
            .iter()
            .position(|card| card == name)
            .map_or(0.0, |rank| {
                (TUTOR_FIRST_BONUS - TUTOR_STEP * rank as f64).max(0.5)
            })
    }

    /// `Some(false)` when the guide says this hand must go back: a seven-card
    /// hand outside the land range or missing a needed group. Only the first
    /// hand is judged — a smaller hand is the registry's call.
    pub fn mulligan_verdict(
        &self,
        state: &GameState,
        hand: &[ObjectId],
        mulligans_taken: u8,
    ) -> Option<bool> {
        let rules = self.mulligan.as_ref()?;
        if mulligans_taken > 0 {
            return None;
        }
        let objects: Vec<_> = hand.iter().filter_map(|id| state.objects.get(id)).collect();
        let lands = objects
            .iter()
            .filter(|obj| obj.card_types.core_types.contains(&CoreType::Land))
            .count() as u8;
        if lands < rules.lands[0] || lands > rules.lands[1] {
            return Some(false);
        }
        let has_group = |group: &Vec<String>| {
            objects
                .iter()
                .any(|obj| group.iter().any(|card| *card == obj.name))
        };
        if !rules.needs_any.iter().all(has_group) {
            return Some(false);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_guide_file_parses_and_every_guide_has_a_signature() {
        let guides = registry();
        assert!(guides.len() >= 17);
        for guide in guides {
            assert!(!guide.signature.is_empty(), "{} has no signature", guide.id);
            if let Some(rules) = &guide.mulligan {
                assert!(rules.lands[0] <= rules.lands[1], "{}", guide.id);
            }
        }
    }

    /// Every card a guide names is in the duel deck it was written for, so a
    /// typo cannot silently switch a rule off.
    #[test]
    fn every_guide_card_is_in_its_duel_deck() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("duel_decks");
        let mut decks = std::collections::HashMap::new();
        for format in std::fs::read_dir(&root).unwrap() {
            for file in std::fs::read_dir(format.unwrap().path()).unwrap() {
                let path = file.unwrap().path();
                let id = path.file_stem().unwrap().to_string_lossy().to_string();
                let json: serde_json::Value =
                    serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
                let names: HashSet<String> = json["main"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|entry| entry["name"].as_str().unwrap().to_string())
                    .collect();
                decks.insert(id, names);
            }
        }
        for guide in registry() {
            let names = decks
                .get(&guide.id)
                .unwrap_or_else(|| panic!("no duel deck {}", guide.id));
            let mut named: Vec<&String> = guide
                .signature
                .iter()
                .chain(&guide.key)
                .chain(&guide.tutor)
                .collect();
            if let Some(rules) = &guide.mulligan {
                named.extend(rules.needs_any.iter().flatten());
            }
            for card in named {
                assert!(
                    names.contains(card),
                    "{}: {card} is not in the deck",
                    guide.id
                );
            }
        }
    }

    #[test]
    fn tutor_bonus_follows_the_guide_order() {
        let guide = registry()
            .iter()
            .find(|guide| guide.id == "amulet-titan")
            .unwrap();
        assert!(guide.tutor_bonus("Primeval Titan") > guide.tutor_bonus("Amulet of Vigor"));
        assert_eq!(guide.tutor_bonus("Lightning Bolt"), 0.0);
    }
}
