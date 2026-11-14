//! Temporada 2026-12 da Mesa do MagicFinder — Domain Zoo (Modern) recusado porque
//! Scion of Draco saía com a lacuna `Swallow:Condition_If`:
//! "Each creature you control has vigilance if it's white, hexproof if it's blue,
//! lifelink if it's black, first strike if it's red, and trample if it's green."
//!
//! O parser já lia a linha como cinco estáticos `Continuous`, um por par, com o
//! filtro de COR (`HasColor`) no `affected` (CR 105.2 + CR 613.1f) — cada "if
//! it's <cor>" é filtro do objeto, não condição de jogo. Só o detector não
//! reconhecia a forma.

use engine::game::keywords::has_keyword;
use engine::game::layers::evaluate_layers;
use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::parser::oracle::parse_oracle_text;
use engine::parser::oracle_ir::diagnostic::OracleDiagnostic;
use engine::types::identifiers::ObjectId;
use engine::types::keywords::Keyword;
use engine::types::mana::ManaColor;
use engine::types::phase::Phase;

const SCION: &str = "Domain — This spell costs {2} less to cast for each basic land type among \
lands you control.\nFlying\nEach creature you control has vigilance if it's white, hexproof if \
it's blue, lifelink if it's black, first strike if it's red, and trample if it's green.";

fn kw(runner: &mut GameRunner, id: ObjectId, keyword: &Keyword) -> bool {
    runner.state_mut().layers_dirty.mark_full();
    evaluate_layers(runner.state_mut());
    has_keyword(&runner.state().objects[&id], keyword)
}

#[test]
fn scion_color_keyword_line_has_no_swallowed_condition() {
    let parsed = parse_oracle_text(
        SCION,
        "Scion of Draco",
        &["Flying".to_string()],
        &["Artifact".to_string(), "Creature".to_string()],
        &["Dragon".to_string()],
    );
    let swallowed: Vec<_> = parsed
        .parse_warnings
        .iter()
        .filter(|w| matches!(w, OracleDiagnostic::SwallowedClause { .. }))
        .collect();
    assert!(swallowed.is_empty(), "got {swallowed:?}");
}

/// An "if" that is NOT a color filter keeps the warning (the exemption is anchored
/// on the whole "has <kw> if it's <color>" list).
#[test]
fn a_non_color_if_on_the_keyword_line_still_warns() {
    let parsed = parse_oracle_text(
        "Each creature you control has vigilance if it's white and trample if you control \
         a Forest.",
        "Test Lord",
        &[],
        &["Creature".to_string()],
        &[],
    );
    let only_color_grants = parsed.parse_warnings.iter().all(|w| {
        !matches!(w, OracleDiagnostic::SwallowedClause { detector, .. } if detector == "Condition_If")
    });
    let fully_unparsed = parsed.statics.is_empty();
    assert!(
        fully_unparsed || !only_color_grants,
        "a game-state \"if\" must not ride on the color-list exemption: {:?} / {:?}",
        parsed.statics,
        parsed.parse_warnings
    );
}

/// CR 105.2 + CR 613.1f: each of your creatures gets the keyword of each of its
/// colors; a multicolored creature gets several, a colorless one none, and the
/// opponent's creatures nothing.
#[test]
fn each_creature_gets_the_keywords_of_its_colors() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let scion = scenario
        .add_creature_from_oracle(P0, "Scion of Draco", 4, 4, SCION)
        .id();
    let white = scenario
        .add_creature(P0, "White Bear", 2, 2)
        .with_color(vec![ManaColor::White])
        .id();
    let boros = scenario
        .add_creature(P0, "Boros Bear", 2, 2)
        .with_color(vec![ManaColor::Red, ManaColor::White])
        .id();
    let sultai = scenario
        .add_creature(P0, "Sultai Bear", 2, 2)
        .with_color(vec![ManaColor::Blue, ManaColor::Black, ManaColor::Green])
        .id();
    let colorless = scenario.add_creature(P0, "Colorless Bear", 2, 2).id();
    let theirs = scenario
        .add_creature(P1, "Their White Bear", 2, 2)
        .with_color(vec![ManaColor::White])
        .id();
    let mut runner = scenario.build();

    assert!(kw(&mut runner, white, &Keyword::Vigilance));
    assert!(!kw(&mut runner, white, &Keyword::Trample));

    assert!(kw(&mut runner, boros, &Keyword::Vigilance));
    assert!(kw(&mut runner, boros, &Keyword::FirstStrike));
    assert!(!kw(&mut runner, boros, &Keyword::Lifelink));

    assert!(kw(&mut runner, sultai, &Keyword::Hexproof));
    assert!(kw(&mut runner, sultai, &Keyword::Lifelink));
    assert!(kw(&mut runner, sultai, &Keyword::Trample));
    assert!(!kw(&mut runner, sultai, &Keyword::Vigilance));

    for kwd in [
        Keyword::Vigilance,
        Keyword::Hexproof,
        Keyword::Lifelink,
        Keyword::FirstStrike,
        Keyword::Trample,
    ] {
        assert!(
            !kw(&mut runner, colorless, &kwd),
            "colorless gets no {kwd:?}"
        );
    }
    assert!(
        !kw(&mut runner, theirs, &Keyword::Vigilance),
        "only creatures you control"
    );
    // Scion itself is colorless (artifact): flying only.
    assert!(kw(&mut runner, scion, &Keyword::Flying));
    assert!(!kw(&mut runner, scion, &Keyword::Vigilance));
}
