//! CR 400.12 + CR 601.2c: "Exile any number of target players' graveyards"
//! (Thraben Charm) empties EVERY chosen player's graveyard — and only theirs.
//!
//! Field report on the released build (2026-09-21): the spell announced its
//! player target, resolved, and the graveyard stayed exactly as it was. The
//! plural possessive missed the mass-zone head, so the clause lowered to
//! `ChangeZone { target: Player }` — an instruction to move ONE object whose
//! target is a player. There is no object to move, so it did nothing, silently.
//!
//! The single-target sibling ("exile target player's graveyard", Bojuka Bog) has
//! always lowered to `ChangeZoneAll`; the parser assertion here pins the plural
//! form to the same shape plus the variable target count, and the runtime half
//! proves the count is honored: two chosen players, two emptied graveyards, and
//! the third player's cards untouched.

use engine::game::scenario::{GameScenario, P0, P1};
use engine::parser::oracle_effect::parse_effect_chain;
use engine::types::ability::{AbilityKind, Effect, MultiTargetSpec, TargetFilter};
use engine::types::phase::Phase;
use engine::types::player::PlayerId;
use engine::types::zones::Zone;

const P2: PlayerId = PlayerId(2);
const ORACLE: &str = "Exile any number of target players' graveyards.";

#[test]
fn exile_any_number_of_target_players_graveyards_is_a_counted_mass_exile() {
    let parsed = parse_effect_chain(ORACLE, AbilityKind::Spell);
    assert!(
        matches!(
            parsed.effect.as_ref(),
            Effect::ChangeZoneAll {
                origin: Some(Zone::Graveyard),
                destination: Zone::Exile,
                target: TargetFilter::Player,
                ..
            }
        ),
        "expected a mass graveyard exile, got {:?}",
        parsed.effect
    );
    assert_eq!(
        parsed.multi_target,
        Some(MultiTargetSpec::unlimited(0)),
        "\"any number of target players\" announces a variable number of player targets"
    );
}

#[test]
fn every_chosen_player_loses_their_graveyard_and_no_one_else_does() {
    let mut scenario = GameScenario::new_n_player(3, 42);
    scenario.at_phase(Phase::PreCombatMain);

    let p0_card = scenario
        .add_creature_to_graveyard(P0, "P0 graveyard card", 1, 1)
        .id();
    let p1_card = scenario
        .add_creature_to_graveyard(P1, "P1 graveyard card", 2, 2)
        .id();
    let p2_card = scenario
        .add_creature_to_graveyard(P2, "P2 graveyard card", 3, 3)
        .id();

    let charm = scenario
        .add_spell_to_hand_from_oracle(P0, "Mass Graveyard Exile", true, ORACLE)
        .id();

    let mut runner = scenario.build();
    let outcome = runner.cast(charm).target_players(&[P0, P1]).resolve();

    assert_eq!(
        outcome.zone_of(p0_card),
        Zone::Exile,
        "the caster chose their own graveyard too — CR 115.1 lets a player target themselves"
    );
    assert_eq!(
        outcome.zone_of(p1_card),
        Zone::Exile,
        "the second chosen player's graveyard must be exiled as well: a single-object \
         move would have stopped after the first target"
    );
    assert_eq!(
        outcome.zone_of(p2_card),
        Zone::Graveyard,
        "an unchosen player keeps their graveyard — the effect is not 'each player'"
    );
}
