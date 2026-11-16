//! Temporada 2026-12 da Mesa do MagicFinder — Lifegain (Standard) recusado porque
//! Case of the Uneaten Feast saía com a lacuna `Swallow:Optional_YouMay`:
//! "Solved — Sacrifice this Case: Creature cards in your graveyard gain 'You may
//! cast this card from your graveyard' until end of turn."
//!
//! O parser lia a habilidade como um `GenericEffect` cujo estático concedia
//! `AddStaticMode(GraveyardCastPermission)` às próprias cartas do cemitério — forma
//! que o sistema de camadas nunca instala numa carta no cemitério: sacrificar o Case
//! não deixava conjurar nada (lacuna de EXECUÇÃO, não só do detector). Agora a
//! concessão sai na forma do Chainer / Gaea's Will: `GrantStaticAbility` ancorado no
//! controlador, com o estático de permissão nomeando as cartas (CR 601.2a + CR 611.2c).

use engine::game::scenario::{GameScenario, P0};
use engine::parser::oracle::parse_oracle_text;
use engine::parser::oracle_ir::diagnostic::OracleDiagnostic;
use engine::types::ability::{ContinuousModification, Effect, TargetFilter, TypeFilter};
use engine::types::actions::GameAction;
use engine::types::game_state::CastPaymentMode;
use engine::types::mana::ManaCost;
use engine::types::phase::Phase;
use engine::types::statics::StaticMode;
use engine::types::zones::Zone;

const CASE: &str = "Whenever a creature you control enters, you gain 1 life.\nTo solve — You've \
gained 5 or more life this turn. (If unsolved, solve at the beginning of your end step.)\n\
Solved — Sacrifice this Case: Creature cards in your graveyard gain \"You may cast this card \
from your graveyard\" until end of turn.";

#[test]
fn case_of_the_uneaten_feast_has_no_swallowed_you_may() {
    let parsed = parse_oracle_text(
        CASE,
        "Case of the Uneaten Feast",
        &[],
        &["Enchantment".to_string()],
        &["Case".to_string()],
    );
    let swallowed: Vec<_> = parsed
        .parse_warnings
        .iter()
        .filter(|w| matches!(w, OracleDiagnostic::SwallowedClause { .. }))
        .collect();
    assert!(swallowed.is_empty(), "got {swallowed:?}");
}

fn cast_from_graveyard(
    runner: &mut engine::game::scenario::GameRunner,
    id: engine::types::identifiers::ObjectId,
) -> bool {
    let card_id = runner.state().objects[&id].card_id;
    runner
        .act(GameAction::CastSpell {
            object_id: id,
            card_id,
            targets: vec![],
            payment_mode: CastPaymentMode::Auto,
        })
        .is_ok()
}

/// CR 719.4 + CR 601.2a: once solved, sacrificing the Case lets you cast the
/// creature cards in your graveyard this turn — and only creature cards.
#[test]
fn solved_case_sacrifice_lets_creature_cards_be_cast_from_the_graveyard() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let case = {
        let mut builder = scenario.add_creature(P0, "Case of the Uneaten Feast", 0, 0);
        builder
            .as_enchantment()
            .with_subtypes(vec!["Case"])
            .from_oracle_text(CASE);
        builder.id()
    };
    let bear = scenario
        .add_creature_to_graveyard(P0, "Graveyard Bear", 2, 2)
        .with_mana_cost(ManaCost::zero())
        .id();
    let mut runner = scenario.build();

    assert!(
        !cast_from_graveyard(&mut runner, bear),
        "no permission before the Case is sacrificed"
    );

    runner
        .state_mut()
        .objects
        .get_mut(&case)
        .unwrap()
        .case_state
        .as_mut()
        .expect("a Case carries case_state")
        .is_solved = true;

    let _ = runner.activate(case, 0).resolve();
    assert_eq!(
        runner.state().objects[&case].zone,
        Zone::Graveyard,
        "the Case was sacrificed"
    );

    assert!(
        cast_from_graveyard(&mut runner, bear),
        "the creature card may now be cast from the graveyard"
    );
    runner.advance_until_stack_empty();
    assert_eq!(runner.state().objects[&bear].zone, Zone::Battlefield);
}

/// The grant covers CREATURE cards only: a noncreature card stays uncastable.
#[test]
fn the_grant_does_not_reach_noncreature_cards() {
    let parsed = parse_oracle_text(
        CASE,
        "Case of the Uneaten Feast",
        &[],
        &["Enchantment".to_string()],
        &["Case".to_string()],
    );
    let Effect::GenericEffect {
        static_abilities, ..
    } = &*parsed.abilities[0].effect
    else {
        panic!(
            "expected the solved ability's GenericEffect: {:?}",
            parsed.abilities
        );
    };
    let [installer] = static_abilities.as_slice() else {
        panic!("one installer static: {static_abilities:?}");
    };
    let [ContinuousModification::GrantStaticAbility { definition }] =
        installer.modifications.as_slice()
    else {
        panic!("controller-anchored grant: {:?}", installer.modifications);
    };
    assert!(matches!(
        definition.mode,
        StaticMode::GraveyardCastPermission { .. }
    ));
    let Some(TargetFilter::Typed(typed)) = &definition.affected else {
        panic!("typed card filter: {:?}", definition.affected);
    };
    assert!(typed.type_filters.contains(&TypeFilter::Creature));
}
