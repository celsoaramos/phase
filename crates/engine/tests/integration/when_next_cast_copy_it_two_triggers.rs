//! CR 707.10 + CR 603.7: two "when you next cast an instant or sorcery spell this
//! turn, copy it" delayed triggers waiting on the same spell (Summon: G.F.
//! Cerberus's chapters II and III in a MagicFinder game). Each must copy the SPELL.
//! Before the fix "it" lowered to `ParentTarget`, the copy fell back to
//! `stack.last()` — the other trigger — and the two triggers copied each other
//! without end: the table never left the main phase.

use engine::game::scenario::{GameScenario, P0, P1};
use engine::types::actions::GameAction;
use engine::types::game_state::WaitingFor;
use engine::types::phase::Phase;

// A PERMANENT's trigger creates the delayed trigger, as Cerberus's chapter does:
// the parent is a triggered ability with a source, not a spell.
const DOUBLE: &str = "When this artifact enters, when you next cast an instant or sorcery spell this turn, copy it. You may choose new targets for the copy.";

/// Pass until the ETB triggers resolve and priority returns on an empty stack.
fn settle(runner: &mut engine::game::scenario::GameRunner) {
    for _ in 0..12 {
        if runner.state().stack.is_empty()
            && matches!(runner.state().waiting_for, WaitingFor::Priority { .. })
        {
            return;
        }
        runner.act(GameAction::PassPriority).expect("pass");
    }
}

#[test]
fn two_next_cast_copy_triggers_copy_the_spell_and_the_stack_empties() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let first = scenario
        .add_artifact_to_hand_from_oracle(P0, "Double A", DOUBLE)
        .id();
    let second = scenario
        .add_artifact_to_hand_from_oracle(P0, "Double B", DOUBLE)
        .id();
    let bolt = scenario.add_bolt_to_hand(P0);
    let mut runner = scenario.build();

    runner.cast(first).commit().resolve();
    settle(&mut runner);
    runner.cast(second).commit().resolve();
    settle(&mut runner);
    let life_before = runner.life(P1);
    let card_id = runner.state().objects[&bolt].card_id;
    runner
        .act(GameAction::CastSpell {
            object_id: bolt,
            card_id,
            targets: vec![],
            payment_mode: Default::default(),
        })
        .expect("cast Bolt");

    // Bolt + two copies, then an empty stack — a bounded walk, not a loop.
    let mut steps = 0;
    while !runner.state().stack.is_empty()
        || !matches!(runner.state().waiting_for, WaitingFor::Priority { .. })
    {
        steps += 1;
        assert!(
            steps < 80,
            "the copy triggers never settled; stack = {:?}",
            runner.stack_names()
        );
        let action = match &runner.state().waiting_for {
            WaitingFor::TargetSelection { .. } => GameAction::ChooseTarget {
                target: Some(engine::types::ability::TargetRef::Player(P1)),
            },
            WaitingFor::OrderTriggers { triggers, .. } => GameAction::OrderTriggers {
                order: (0..triggers.len()).collect(),
            },
            WaitingFor::CopyRetarget { .. } => GameAction::KeepAllCopyTargets,
            WaitingFor::Priority { .. } => GameAction::PassPriority,
            other => panic!("unexpected wait: {other:?}"),
        };
        runner.act(action).expect("legal step");
        if runner.state().stack.is_empty()
            && matches!(runner.state().waiting_for, WaitingFor::Priority { .. })
        {
            break;
        }
    }
    assert_eq!(
        life_before - runner.life(P1),
        9,
        "Bolt and both copies deal 3 each"
    );
}
