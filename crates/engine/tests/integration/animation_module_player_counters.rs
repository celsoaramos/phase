//! Animation Module — "{3}, {T}: Choose a counter on target permanent or
//! player. Give that permanent or player another counter of that kind."
//!
//! CR 122.1: players can have counters (poison, energy, experience, …), so the
//! counter-kind choice offers the kinds on whichever was targeted, and the
//! extra counter goes to that permanent or player. CR 608.2d: the choice is
//! made as the ability resolves; one kind is chosen without a prompt, and a
//! target with no counters offers nothing.
//!
//! Every test activates the real ability through the activation pipeline.

use engine::ai_support::candidate_actions;
use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::types::ability::{ChoiceType, Effect};
use engine::types::actions::GameAction;
use engine::types::counter::CounterType;
use engine::types::game_state::WaitingFor;
use engine::types::identifiers::ObjectId;
use engine::types::mana::{ManaType, ManaUnit};
use engine::types::phase::Phase;
use engine::types::player::{PlayerCounterKind, PlayerId};

const ANIMATION_MODULE: &str = "Whenever one or more +1/+1 counters are put on a permanent you \
    control, you may pay {1}. If you do, create a 1/1 colorless Servo artifact creature token.\n\
    {3}, {T}: Choose a counter on target permanent or player. Give that permanent or player \
    another counter of that kind.";

/// Animation Module under P0's control with {3} floating, plus a P1 creature.
/// The creature belongs to P1 so Animation Module's own "+1/+1 counters are
/// put on a permanent you control" trigger stays out of the way.
fn setup(creature_counters: &[(CounterType, u32)]) -> (GameRunner, ObjectId, ObjectId) {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let module = scenario
        .add_artifact_from_oracle(P0, "Animation Module", ANIMATION_MODULE)
        .id();
    let creature = scenario.add_creature(P1, "Counter Holder", 2, 2).id();
    for (counter_type, count) in creature_counters {
        scenario.with_counter(creature, counter_type.clone(), *count);
    }
    scenario.with_mana_pool(
        P0,
        (0..3)
            .map(|_| ManaUnit::new(ManaType::Colorless, ObjectId(0), false, vec![]))
            .collect(),
    );
    (scenario.build(), module, creature)
}

/// The module's activated ability (its trigger is not an activated ability).
fn activated_index(runner: &GameRunner, module: ObjectId) -> usize {
    let abilities = &runner.state().objects[&module].abilities;
    assert_eq!(abilities.len(), 1, "one activated ability: {abilities:#?}");
    0
}

fn object_counters(runner: &GameRunner, object: ObjectId, counter_type: &CounterType) -> u32 {
    runner.state().objects[&object]
        .counters
        .get(counter_type)
        .copied()
        .unwrap_or(0)
}

fn poison(runner: &GameRunner, player: PlayerId) -> u32 {
    runner.state().players[player.0 as usize].poison_counters
}

fn energy(runner: &GameRunner, player: PlayerId) -> u32 {
    runner.state().players[player.0 as usize].energy
}

fn counter_kind_options(waiting_for: &WaitingFor) -> Vec<String> {
    match waiting_for {
        WaitingFor::NamedChoice {
            player,
            choice_type,
            options,
            ..
        } => {
            assert_eq!(*player, P0, "the module's controller chooses the kind");
            assert!(
                matches!(choice_type, ChoiceType::CounterKind { .. }),
                "expected a counter-kind choice, got {choice_type:?}"
            );
            options.clone()
        }
        other => panic!("expected a counter-kind NamedChoice, got {other:?}"),
    }
}

/// The parsed chain: designate the target, choose a kind on it, then give it
/// another counter of that kind.
#[test]
fn animation_module_parses_to_a_kind_choice_on_its_target() {
    let (runner, module, _) = setup(&[]);
    let ability = &runner.state().objects[&module].abilities[activated_index(&runner, module)];
    assert!(matches!(&*ability.effect, Effect::TargetOnly { .. }));
    let choose = ability.sub_ability.as_deref().expect("kind choice follows");
    assert!(
        matches!(&*choose.effect, Effect::ChooseCounterKind { .. }),
        "got {:?}",
        choose.effect
    );
    let give = choose.sub_ability.as_deref().expect("placement follows");
    assert!(
        matches!(&*give.effect, Effect::PutChosenCounter { .. }),
        "got {:?}",
        give.effect
    );
}

/// (a) CR 122.1 + CR 608.2d: one kind on the permanent is chosen without a
/// prompt, and the permanent gets another counter of it.
#[test]
fn a_permanent_with_plus_one_counters_gets_another() {
    let (mut runner, module, creature) = setup(&[(CounterType::Plus1Plus1, 2)]);
    let index = activated_index(&runner, module);
    let outcome = runner
        .activate(module, index)
        .target_object(creature)
        .resolve();

    assert!(matches!(
        outcome.final_waiting_for(),
        WaitingFor::Priority { .. }
    ));
    assert_eq!(
        object_counters(&runner, creature, &CounterType::Plus1Plus1),
        3
    );
    assert!(runner.state().objects[&module].tapped, "{{T}} was paid");
}

/// (b) CR 122.1 + CR 122.1f: a player with poison counters gets another one.
#[test]
fn a_poisoned_player_gets_another_poison_counter() {
    let (mut runner, module, _) = setup(&[]);
    runner.state_mut().players[1].poison_counters = 2;
    let index = activated_index(&runner, module);
    let outcome = runner.activate(module, index).target_player(P1).resolve();

    assert!(matches!(
        outcome.final_waiting_for(),
        WaitingFor::Priority { .. }
    ));
    assert_eq!(poison(&runner, P1), 3);
    assert_eq!(poison(&runner, P0), 0);
}

/// (b) CR 122.1: energy is a player counter too, even though the engine keeps
/// it apart from the named kinds.
#[test]
fn a_player_with_energy_gets_another_energy_counter() {
    let (mut runner, module, _) = setup(&[]);
    runner.state_mut().players[0].energy = 2;
    let index = activated_index(&runner, module);
    runner.activate(module, index).target_player(P0).resolve();

    assert_eq!(energy(&runner, P0), 3);
}

/// (b) CR 122.1: a named player counter other than poison (experience).
#[test]
fn a_player_with_experience_gets_another_experience_counter() {
    let (mut runner, module, _) = setup(&[]);
    runner.state_mut().players[0].add_player_counters(&PlayerCounterKind::Experience, 1);
    let index = activated_index(&runner, module);
    runner.activate(module, index).target_player(P0).resolve();

    assert_eq!(
        runner.state().players[0].player_counter(&PlayerCounterKind::Experience),
        2
    );
}

/// (c) CR 608.2d: a target with no counters offers no kind — no prompt, and
/// nothing is added.
#[test]
fn a_target_without_counters_asks_nothing_and_adds_nothing() {
    let (mut runner, module, creature) = setup(&[]);
    let index = activated_index(&runner, module);
    let outcome = runner
        .activate(module, index)
        .target_object(creature)
        .resolve();
    assert!(matches!(
        outcome.final_waiting_for(),
        WaitingFor::Priority { .. }
    ));
    assert!(runner.state().objects[&creature].counters.is_empty());

    let (mut runner, module, _) = setup(&[]);
    let outcome = runner.activate(module, index).target_player(P1).resolve();
    assert!(matches!(
        outcome.final_waiting_for(),
        WaitingFor::Priority { .. }
    ));
    assert_eq!(poison(&runner, P1), 0);
    assert_eq!(energy(&runner, P1), 0);
}

/// (c) CR 608.2d: a permanent with two kinds asks which one; only the chosen
/// kind grows.
#[test]
fn a_permanent_with_two_kinds_asks_which_and_adds_only_that_kind() {
    let (mut runner, module, creature) =
        setup(&[(CounterType::Plus1Plus1, 1), (CounterType::Stun, 1)]);
    let index = activated_index(&runner, module);
    let outcome = runner
        .activate(module, index)
        .target_object(creature)
        .resolve();

    let mut options = counter_kind_options(outcome.final_waiting_for());
    options.sort();
    let mut expected = vec![
        CounterType::Plus1Plus1.as_str().into_owned(),
        CounterType::Stun.as_str().into_owned(),
    ];
    expected.sort();
    assert_eq!(options, expected);

    runner
        .act(GameAction::ChooseOption {
            choice: CounterType::Stun.as_str().into_owned(),
        })
        .expect("choosing a kind on the permanent succeeds");
    assert_eq!(object_counters(&runner, creature, &CounterType::Stun), 2);
    assert_eq!(
        object_counters(&runner, creature, &CounterType::Plus1Plus1),
        1
    );
}

/// (c) CR 122.1 + CR 608.2d: a player with two kinds (poison and energy) is
/// asked about the player's kinds — never a permanent's — and only the chosen
/// kind grows.
#[test]
fn a_player_with_two_kinds_asks_which_and_adds_only_that_kind() {
    let (mut runner, module, _) = setup(&[(CounterType::Plus1Plus1, 1)]);
    runner.state_mut().players[1].poison_counters = 4;
    runner.state_mut().players[1].energy = 1;
    let index = activated_index(&runner, module);
    let outcome = runner.activate(module, index).target_player(P1).resolve();

    assert_eq!(
        counter_kind_options(outcome.final_waiting_for()),
        vec!["poison".to_string(), "energy".to_string()]
    );

    runner
        .act(GameAction::ChooseOption {
            choice: "energy".to_string(),
        })
        .expect("choosing a kind on the player succeeds");
    assert_eq!(energy(&runner, P1), 2);
    assert_eq!(poison(&runner, P1), 4);
}

/// (d) The AI's candidate domain for the player counter-kind prompt is exactly
/// the offered kinds, and every candidate settles without an engine error.
#[test]
fn every_ai_candidate_for_a_player_counter_choice_settles() {
    for choice in ["poison", "energy"] {
        let (mut runner, module, _) = setup(&[]);
        runner.state_mut().players[1].poison_counters = 1;
        runner.state_mut().players[1].energy = 1;
        let index = activated_index(&runner, module);
        runner.activate(module, index).target_player(P1).resolve();

        let candidates: Vec<GameAction> = candidate_actions(runner.state())
            .into_iter()
            .map(|candidate| candidate.action)
            .collect();
        let offered: Vec<String> = candidates
            .iter()
            .filter_map(|action| match action {
                GameAction::ChooseOption { choice } => Some(choice.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(offered, vec!["poison".to_string(), "energy".to_string()]);

        let action = candidates
            .into_iter()
            .find(|action| matches!(action, GameAction::ChooseOption { choice: c } if c == choice))
            .expect("the candidate is offered");
        runner.act(action).expect("the AI's candidate settles");
        assert!(matches!(
            runner.state().waiting_for,
            WaitingFor::Priority { .. }
        ));
        let (poison_after, energy_after) = (poison(&runner, P1), energy(&runner, P1));
        match choice {
            "poison" => assert_eq!((poison_after, energy_after), (2, 1)),
            _ => assert_eq!((poison_after, energy_after), (1, 2)),
        }
    }
}
