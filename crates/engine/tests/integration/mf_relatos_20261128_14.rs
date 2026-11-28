//! Relatos da mesa de 28/11/2026.
//!
//! Cascading Cataracts — "{5}, {T}: Add five mana in any combination of colors."
//! The manual tap only offered one planner row per color, each adding all five
//! units of that color. CR 106.4: each unit of "any combination of colors" is
//! chosen independently, so a manual tap must be able to ask for the combination.

use std::sync::Arc;

use engine::game::mana_sources::activatable_mana_actions_for_player;
use engine::game::scenario::{GameScenario, P0};
use engine::types::ability::{
    AbilityCost, AbilityDefinition, AbilityKind, Effect, ManaProduction, QuantityExpr,
};
use engine::types::actions::GameAction;
use engine::types::card_type::CoreType;
use engine::types::game_state::{ManaChoice, ManaChoicePrompt, WaitingFor};
use engine::types::identifiers::ObjectId;
use engine::types::mana::{ManaColor, ManaSourceOutput, ManaType};
use engine::types::phase::Phase;

fn any_combination_land(scenario: &mut GameScenario, count: i32) -> ObjectId {
    scenario
        .add_creature(P0, "Combination Land", 0, 0)
        .with_ability_definition(
            AbilityDefinition::new(
                AbilityKind::Activated,
                Effect::Mana {
                    produced: ManaProduction::AnyCombination {
                        count: QuantityExpr::Fixed { value: count },
                        color_options: ManaColor::ALL.to_vec(),
                    },
                    restrictions: vec![],
                    grants: vec![],
                    expiry: None,
                    target: None,
                },
            )
            .cost(AbilityCost::Tap),
        )
        .id()
}

fn make_land(runner: &mut engine::game::scenario::GameRunner, land: ObjectId) {
    let obj = runner.state_mut().objects.get_mut(&land).unwrap();
    obj.card_types.core_types = vec![CoreType::Land];
    obj.base_card_types = obj.card_types.clone();
    obj.power = None;
    obj.toughness = None;
    obj.base_power = None;
    obj.base_toughness = None;
    Arc::make_mut(&mut obj.abilities);
}

fn land_rows(state: &engine::types::game_state::GameState, land: ObjectId) -> Vec<GameAction> {
    activatable_mana_actions_for_player(state, P0)
        .into_iter()
        .filter(|action| {
            matches!(action, GameAction::TapLandForMana { selection }
                if selection.source.object_id == land)
        })
        .collect()
}

#[test]
fn cascading_cataracts_manual_tap_chooses_each_unit() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let land = any_combination_land(&mut scenario, 5);
    let mut runner = scenario.build();
    make_land(&mut runner, land);

    let rows = land_rows(runner.state(), land);
    let deferred: Vec<_> = rows
        .iter()
        .filter(|action| {
            matches!(action, GameAction::TapLandForMana { selection }
                if matches!(selection.output, ManaSourceOutput::DeferredColorChoice { .. }))
        })
        .cloned()
        .collect();
    assert_eq!(deferred.len(), 1, "one combination row, got {rows:?}");
    assert!(
        rows.len() > 1,
        "the planner's per-color rows stay available"
    );

    runner
        .act(deferred[0].clone())
        .expect("the combination row taps the land");
    match &runner.state().waiting_for {
        WaitingFor::ChooseManaColor {
            choice: ManaChoicePrompt::AnyCombination { count, options },
            ..
        } => {
            assert_eq!(*count, 5);
            assert_eq!(options.len(), 5);
        }
        other => panic!("expected the any-combination prompt, got {other:?}"),
    }

    runner
        .act(GameAction::ChooseManaColor {
            choice: ManaChoice::Combination(vec![
                ManaType::White,
                ManaType::Blue,
                ManaType::Black,
                ManaType::Green,
                ManaType::Green,
            ]),
            count: 1,
        })
        .expect("any combination of the five colors is legal");

    let pool = &runner.state().players[P0.0 as usize].mana_pool;
    assert_eq!(pool.count_color(ManaType::White), 1);
    assert_eq!(pool.count_color(ManaType::Blue), 1);
    assert_eq!(pool.count_color(ManaType::Black), 1);
    assert_eq!(pool.count_color(ManaType::Red), 0);
    assert_eq!(pool.count_color(ManaType::Green), 2);
    assert_eq!(pool.total(), 5);
    assert!(runner.state().objects[&land].tapped);
}

#[test]
fn single_unit_combination_keeps_only_concrete_rows() {
    // "Add one mana in any combination of colors" is just one color: the
    // per-color rows already name the whole output.
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let land = any_combination_land(&mut scenario, 1);
    let mut runner = scenario.build();
    make_land(&mut runner, land);

    let rows = land_rows(runner.state(), land);
    assert!(!rows.is_empty());
    assert!(rows.iter().all(|action| matches!(action,
        GameAction::TapLandForMana { selection }
            if matches!(selection.output, ManaSourceOutput::Concrete(_)))));
}
