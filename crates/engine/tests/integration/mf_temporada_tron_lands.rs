//! Temporada 2026-12 da Mesa do MagicFinder — Eldrazi Tron / Eldrazi Ramp (Modern)
//! recusados porque Urza's Mine, Urza's Power Plant, Urza's Tower e Ugin's Labyrinth
//! saíam com a lacuna `Swallow:Replacement_Instead`.
//!
//! O motor já produzia a mana certa: `normalize_activated_mana_instead_delta`
//! dobra "{T}: Add {C}. If <cond>, add {C}{C} instead." num `Mana` incolor fixo
//! com um sub-`Mana` incolor CONDICIONADO à diferença (CR 106.1 + CR 614.1a —
//! base + delta = o valor de "instead" num reservatório do mesmo tipo). Só o
//! detector de "instead" não reconhecia essa forma e marcava a carta inteira como
//! não suportada.

use engine::game::scenario::{GameScenario, P0};
use engine::game::scenario_db::GameScenarioDbExt;
use engine::parser::oracle::parse_oracle_text;
use engine::parser::oracle_ir::diagnostic::OracleDiagnostic;
use engine::types::mana::ManaType;
use engine::types::phase::Phase;
use engine::types::zones::Zone;

use crate::support::shared_card_db as load_db;

const CARDS: &[(&str, &str, &[&str])] = &[
    (
        "Urza's Mine",
        "{T}: Add {C}. If you control an Urza's Power-Plant and an Urza's Tower, add {C}{C} instead.",
        &["Urza's", "Mine"],
    ),
    (
        "Urza's Power Plant",
        "{T}: Add {C}. If you control an Urza's Mine and an Urza's Tower, add {C}{C} instead.",
        &["Urza's", "Power-Plant"],
    ),
    (
        "Urza's Tower",
        "{T}: Add {C}. If you control an Urza's Mine and an Urza's Power-Plant, add {C}{C}{C} instead.",
        &["Urza's", "Tower"],
    ),
    (
        "Ugin's Labyrinth",
        "Imprint — When this land enters, you may exile a colorless card with mana value 7 or \
         greater from your hand.\n{T}: Add {C}. If a card is exiled with this land, add {C}{C} \
         instead.\n{T}: Return the exiled card to its owner's hand.",
        &[],
    ),
];

fn swallowed(name: &str, text: &str, subtypes: &[&str]) -> Vec<String> {
    let subtypes: Vec<String> = subtypes.iter().map(|s| s.to_string()).collect();
    parse_oracle_text(text, name, &[], &["Land".to_string()], &subtypes)
        .parse_warnings
        .iter()
        .filter_map(|w| match w {
            OracleDiagnostic::SwallowedClause { detector, .. } => Some(detector.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn tron_lands_and_labyrinth_have_no_swallowed_instead_clause() {
    for (name, text, subtypes) in CARDS {
        let detectors = swallowed(name, text, subtypes);
        assert!(
            detectors.is_empty(),
            "{name}: the conditional colorless delta represents \"instead\"; got {detectors:?}"
        );
    }
}

/// CR 605.3b: mana abilities resolve on activation. Mine alone → {C}; with the
/// other two Urza lands → {C}{C}; Power Plant likewise.
#[test]
fn urzas_mine_and_power_plant_produce_two_only_with_the_full_set() {
    let Some(db) = load_db() else {
        return;
    };

    let mut lonely = GameScenario::new();
    lonely.at_phase(Phase::PreCombatMain);
    let mine = lonely.add_real_card(P0, "Urza's Mine", Zone::Battlefield, db);
    let mut runner = lonely.build();
    let outcome = runner.activate(mine, 0).resolve();
    assert_eq!(outcome.mana_pool_color(P0, ManaType::Colorless), 1);

    for land in ["Urza's Mine", "Urza's Power Plant"] {
        let mut scenario = GameScenario::new();
        scenario.at_phase(Phase::PreCombatMain);
        let mut ids = Vec::new();
        for name in ["Urza's Mine", "Urza's Power Plant", "Urza's Tower"] {
            ids.push((
                name,
                scenario.add_real_card(P0, name, Zone::Battlefield, db),
            ));
        }
        let id = ids.iter().find(|(n, _)| *n == land).unwrap().1;
        let mut runner = scenario.build();
        let outcome = runner.activate(id, 0).resolve();
        assert_eq!(
            outcome.mana_pool_color(P0, ManaType::Colorless),
            2,
            "{land} with the other two Urza lands adds {{C}}{{C}}"
        );
        assert_eq!(outcome.mana_pool_total(P0), 2);
    }
}
