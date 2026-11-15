//! "You may draw a card unless that player pays {N}" (Rhystic Study, Mystic
//! Remora): accepting costs the AI nothing — the payment, if any, comes out of
//! the OPPONENT's mana — so declining throws a free card (or a tax on the
//! opponent) away. Measured on v0.110.0 against MagicFinder's deck base: the AI
//! that owned Rhystic Study declined the draw on most triggers (Medium scored
//! accept 4 points below decline; VeryHard was a coin flip).

use engine::game::scenario::{GameScenario, P0, P1};
use engine::types::actions::GameAction;
use engine::types::game_state::WaitingFor;
use engine::types::mana::ManaColor;
use engine::types::phase::Phase;
use phase_ai::choose_action;
use phase_ai::config::{create_config, AiDifficulty, Platform};
use rand::rngs::SmallRng;
use rand::SeedableRng;

fn trigger_choice(name: &str, oracle: &str) -> engine::game::scenario::GameRunner {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.add_enchantment_from_oracle(P0, name, oracle);
    for _ in 0..3 {
        scenario.add_basic_land(P0, ManaColor::Blue);
    }
    for _ in 0..3 {
        scenario.add_basic_land(P1, ManaColor::Red);
    }
    let bolt = scenario.add_bolt_to_hand(P1);
    scenario.with_library_top(
        P0,
        &["Island", "Island", "Island", "Island", "Island", "Island"],
    );
    let mut runner = scenario.build();
    runner.state_mut().active_player = P1;
    runner.state_mut().priority_player = P1;
    runner.state_mut().waiting_for = WaitingFor::Priority { player: P1 };
    drop(runner.cast(bolt).target_player(P0).commit());
    // The cast trigger goes on the stack above Bolt; pass until the owner is asked.
    for _ in 0..8 {
        if matches!(
            runner.state().waiting_for,
            WaitingFor::OptionalEffectChoice { .. }
        ) {
            return runner;
        }
        let who = match &runner.state().waiting_for {
            WaitingFor::Priority { player } => *player,
            other => panic!("unexpected wait before the may-draw choice: {other:?}"),
        };
        runner
            .act(GameAction::PassPriority)
            .unwrap_or_else(|e| panic!("{who:?} pass: {e:?}"));
    }
    panic!(
        "the trigger never asked its owner: {:?}",
        runner.state().waiting_for
    );
}

fn assert_accepts(name: &str, oracle: &str) {
    let runner = trigger_choice(name, oracle);
    // Easy scores every option 0.5 by design (a uniform pick); the policy layer
    // is what Medium and up consult.
    for difficulty in [
        AiDifficulty::Medium,
        AiDifficulty::Hard,
        AiDifficulty::VeryHard,
    ] {
        let config = create_config(difficulty, Platform::Native);
        for seed in 0..6 {
            let mut rng = SmallRng::seed_from_u64(seed);
            let action = choose_action(runner.state(), P0, &config, &mut rng).expect("AI acts");
            assert_eq!(
                action,
                GameAction::DecideOptionalEffect { accept: true },
                "{name}: {difficulty:?} seed {seed} declined a free draw"
            );
        }
    }
}

#[test]
fn rhystic_study_owner_accepts_the_draw() {
    assert_accepts(
        "Rhystic Study",
        "Whenever an opponent casts a spell, you may draw a card unless that player pays {1}.",
    );
}

#[test]
fn mystic_remora_owner_accepts_the_draw() {
    assert_accepts(
        "Mystic Remora",
        "Whenever an opponent casts a noncreature spell, you may draw a card unless that player pays {4}.",
    );
}

/// No opponent payment involved: the projection scored accept and decline the
/// SAME for Aether Vial's charge counter, and the tie went to decline — a bot
/// running Vial never charged it.
#[test]
fn aether_vial_owner_charges_it_at_upkeep() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::Untap);
    scenario.add_artifact_from_oracle(
        P0,
        "Aether Vial",
        "At the beginning of your upkeep, you may put a charge counter on this artifact.\n{T}: You may put a creature card with mana value equal to the number of charge counters on this artifact from your hand onto the battlefield.",
    );
    scenario.with_library_top(P0, &["Island", "Island", "Island", "Island", "Island"]);
    let mut runner = scenario.build();
    for _ in 0..8 {
        if matches!(
            runner.state().waiting_for,
            WaitingFor::OptionalEffectChoice { .. }
        ) {
            break;
        }
        runner
            .act(GameAction::PassPriority)
            .expect("pass to the upkeep trigger");
    }
    assert!(
        matches!(
            runner.state().waiting_for,
            WaitingFor::OptionalEffectChoice { .. }
        ),
        "Vial's upkeep trigger never asked: {:?}",
        runner.state().waiting_for
    );
    // A preference, not a reject (charging Vial past the curve can be wrong):
    // the softmax still draws, so the bar is "almost always", per temperature.
    for (difficulty, min_accepts) in [(AiDifficulty::Medium, 14), (AiDifficulty::VeryHard, 20)] {
        let config = create_config(difficulty, Platform::Native);
        let accepts = (0..20)
            .filter(|&seed| {
                let mut rng = SmallRng::seed_from_u64(seed);
                choose_action(runner.state(), P0, &config, &mut rng)
                    == Some(GameAction::DecideOptionalEffect { accept: true })
            })
            .count();
        assert!(
            accepts >= min_accepts,
            "Aether Vial: {difficulty:?} charged it {accepts}/20 times (want >= {min_accepts})"
        );
    }
}
