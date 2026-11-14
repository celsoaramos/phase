//! Animation Module — "{3}, {T}: Choose a counter on target permanent or
//! player. Give that permanent or player another counter of that kind."
//!
//! CR 122.1 + CR 608.2d: the counter-kind choice can now be about a player's
//! counters (poison, energy, …). The AI must pick one of the offered kinds,
//! and driving the AI from priority with the module on the battlefield must
//! not stall or panic.

use engine::game::engine::apply_as_current;
use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::types::actions::GameAction;
use engine::types::game_state::WaitingFor;
use engine::types::identifiers::ObjectId;
use engine::types::mana::{ManaType, ManaUnit};
use engine::types::phase::Phase;
use phase_ai::choose_action;
use phase_ai::config::AiConfig;
use rand::rngs::SmallRng;
use rand::SeedableRng;

const ANIMATION_MODULE: &str = "Whenever one or more +1/+1 counters are put on a permanent you \
    control, you may pay {1}. If you do, create a 1/1 colorless Servo artifact creature token.\n\
    {3}, {T}: Choose a counter on target permanent or player. Give that permanent or player \
    another counter of that kind.";

/// P0 controls Animation Module with {3} floating; P1 has poison and energy.
fn setup() -> (GameRunner, ObjectId) {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let module = scenario
        .add_artifact_from_oracle(P0, "Animation Module", ANIMATION_MODULE)
        .id();
    scenario.with_mana_pool(
        P0,
        (0..3)
            .map(|_| ManaUnit::new(ManaType::Colorless, ObjectId(0), false, vec![]))
            .collect(),
    );
    let mut runner = scenario.build();
    runner.state_mut().players[1].poison_counters = 3;
    runner.state_mut().players[1].energy = 2;
    (runner, module)
}

/// The AI answers a player counter-kind prompt with one of the offered kinds,
/// and the answer settles.
#[test]
fn ai_picks_an_offered_player_counter_kind() {
    let (mut runner, module) = setup();
    runner.activate(module, 0).target_player(P1).resolve();
    let WaitingFor::NamedChoice { options, .. } = runner.state().waiting_for.clone() else {
        panic!(
            "expected the counter-kind prompt, got {:?}",
            runner.state().waiting_for
        );
    };
    assert_eq!(options, vec!["poison".to_string(), "energy".to_string()]);

    let mut rng = SmallRng::seed_from_u64(7);
    let action = choose_action(runner.state(), P0, &AiConfig::default(), &mut rng)
        .expect("the AI answers the prompt");
    let GameAction::ChooseOption { choice } = &action else {
        panic!("expected ChooseOption, got {action:?}");
    };
    assert!(options.contains(choice), "{choice} was not offered");

    apply_as_current(runner.state_mut(), action).expect("the AI's answer settles");
    assert!(matches!(
        runner.state().waiting_for,
        WaitingFor::Priority { .. }
    ));
    let p1 = &runner.state().players[1];
    assert_eq!(
        p1.poison_counters + p1.energy,
        6,
        "exactly one counter of the chosen kind was added"
    );
}

/// Driving the AI from priority with the module ready never panics, and every
/// action it picks is accepted by the engine.
#[test]
fn ai_drives_a_turn_with_animation_module_without_errors() {
    let (mut runner, _) = setup();
    let mut rng = SmallRng::seed_from_u64(11);
    for _ in 0..40 {
        let Some(player) = runner.state().waiting_for.acting_player() else {
            break;
        };
        if runner.state().phase != Phase::PreCombatMain {
            break;
        }
        let Some(action) = choose_action(runner.state(), player, &AiConfig::default(), &mut rng)
        else {
            break;
        };
        apply_as_current(runner.state_mut(), action.clone())
            .unwrap_or_else(|error| panic!("the engine rejected the AI's {action:?}: {error:?}"));
    }
}
