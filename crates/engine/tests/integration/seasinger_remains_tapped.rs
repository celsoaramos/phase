//! Seasinger — three defects from one field report (MagicFinder, 2026-10-20).
//!
//! "You may choose not to untap Seasinger during your untap step.
//! When you control no Islands, sacrifice Seasinger.
//! {T}: Gain control of target creature whose controller controls an Island for
//! as long as you control Seasinger and Seasinger remains tapped."
//!
//! 1. CR 115.1: the target must be a creature whose controller controls an
//!    Island. The relative clause was dropped, so any creature was stealable.
//! 2. CR 611.2b: if Seasinger untaps while its ability waits on the stack, the
//!    "remains tapped" duration has ended before the effect could begin — the
//!    ability does nothing, even when Seasinger is tapped again (a second
//!    activation) before it resolves. Untapping and re-activating used to keep
//!    BOTH creatures.
//! 3. CR 502.3: "you may choose not to untap" — the decline was missing from the
//!    legal actions, so the player could only untap and lose the creature.

use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::types::ability::TargetRef;
use engine::types::actions::GameAction;
use engine::types::game_state::WaitingFor;
use engine::types::identifiers::ObjectId;
use engine::types::mana::ManaColor;
use engine::types::phase::Phase;

const SEASINGER_ORACLE: &str = "You may choose not to untap Seasinger during your untap step.\nWhen you control no Islands, sacrifice Seasinger.\n{T}: Gain control of target creature whose controller controls an Island for as long as you control Seasinger and Seasinger remains tapped.";
const FOLLOWER_ORACLE: &str = "{T}: Untap another target permanent.";

struct Table {
    runner: GameRunner,
    seasinger: ObjectId,
    follower: ObjectId,
    first: ObjectId,
    second: ObjectId,
}

fn table(opponent_has_island: bool) -> Table {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.add_basic_land(P0, ManaColor::Blue);
    let seasinger = scenario
        .add_creature_from_oracle(P0, "Seasinger", 0, 1, SEASINGER_ORACLE)
        .with_subtypes(vec!["Merfolk"])
        .id();
    let follower = scenario
        .add_creature_from_oracle(P0, "Kiora's Follower", 2, 2, FOLLOWER_ORACLE)
        .id();
    scenario.add_basic_land(
        P1,
        if opponent_has_island {
            ManaColor::Blue
        } else {
            ManaColor::Green
        },
    );
    let first = scenario.add_creature(P1, "Grizzly Bears", 2, 2).id();
    // Libraries, so the turn cycle in the untap-step test does not deck anyone.
    for _ in 0..4 {
        scenario.add_card_to_library_top(P0, "Filler");
        scenario.add_card_to_library_top(P1, "Filler");
    }
    let second = scenario.add_creature(P1, "Giant Spider", 2, 4).id();
    Table {
        runner: scenario.build(),
        seasinger,
        follower,
        first,
        second,
    }
}

/// Put `source`'s first ability on the stack aimed at `target`, leaving it there.
fn activate_at(runner: &mut GameRunner, source: ObjectId, target: ObjectId) {
    runner
        .act(GameAction::ActivateAbility {
            source_id: source,
            ability_index: 0,
        })
        .expect("activate");
    for _ in 0..8 {
        match runner.state().waiting_for.clone() {
            WaitingFor::TargetSelection { .. } => {
                runner
                    .act(GameAction::ChooseTarget {
                        target: Some(TargetRef::Object(target)),
                    })
                    .expect("choose target");
            }
            WaitingFor::Priority { .. } => return,
            other => panic!("unexpected state while activating: {other:?}"),
        }
    }
    panic!("activation never returned to priority");
}

/// The creatures Seasinger's activation may target (CR 115.1).
fn seasinger_legal_targets(runner: &mut GameRunner, seasinger: ObjectId) -> Vec<TargetRef> {
    runner
        .act(GameAction::ActivateAbility {
            source_id: seasinger,
            ability_index: 0,
        })
        .expect("activate Seasinger");
    match runner.state().waiting_for.clone() {
        WaitingFor::TargetSelection { selection, .. } => selection.current_legal_targets,
        other => panic!("expected a target choice, got {other:?}"),
    }
}

#[test]
fn seasinger_needs_a_creature_whose_controller_controls_an_island() {
    // P0 controls an Island too, so its own creatures stay legal targets (the
    // prompt still opens); the opponent's creatures only with an Island.
    let Table {
        mut runner,
        seasinger,
        first,
        second,
        ..
    } = table(false);
    let legal = seasinger_legal_targets(&mut runner, seasinger);
    assert!(
        !legal.contains(&TargetRef::Object(first)) && !legal.contains(&TargetRef::Object(second)),
        "the opponent controls no Island: none of their creatures is a legal target, got {legal:?}"
    );

    let Table {
        mut runner,
        seasinger,
        first,
        second,
        ..
    } = table(true);
    let legal = seasinger_legal_targets(&mut runner, seasinger);
    assert!(
        legal.contains(&TargetRef::Object(first)) && legal.contains(&TargetRef::Object(second)),
        "reach-guard: with an Island their creatures are legal targets, got {legal:?}"
    );
}

#[test]
fn seasinger_steals_while_it_stays_tapped() {
    let Table {
        mut runner,
        seasinger,
        first,
        ..
    } = table(true);
    activate_at(&mut runner, seasinger, first);
    runner.advance_until_stack_empty();
    assert_eq!(
        runner.state().objects[&first].controller,
        P0,
        "controller of {}",
        stringify!(first)
    );
}

#[test]
fn untapping_seasinger_in_response_voids_the_first_activation() {
    let Table {
        mut runner,
        seasinger,
        follower,
        first,
        second,
    } = table(true);
    activate_at(&mut runner, seasinger, first);
    // In response: untap Seasinger and resolve only that, then activate again.
    activate_at(&mut runner, follower, seasinger);
    runner.resolve_top();
    assert!(
        !runner.state().objects[&seasinger].tapped,
        "reach-guard: Kiora's Follower untapped Seasinger"
    );
    activate_at(&mut runner, seasinger, second);
    runner.advance_until_stack_empty();

    assert_eq!(
        runner.state().objects[&second].controller,
        P0,
        "controller of {}",
        stringify!(second)
    );
    assert_eq!(
        runner.state().objects[&first].controller,
        P1,
        "controller of {}",
        stringify!(first)
    );
}

#[test]
fn seasinger_may_stay_tapped_in_the_untap_step() {
    let Table {
        mut runner,
        seasinger,
        first,
        ..
    } = table(true);
    activate_at(&mut runner, seasinger, first);
    runner.advance_until_stack_empty();
    assert_eq!(
        runner.state().objects[&first].controller,
        P0,
        "controller of {}",
        stringify!(first)
    );

    // Drive the turns until P0's untap step asks about Seasinger.
    for _ in 0..200 {
        match runner.state().waiting_for.clone() {
            WaitingFor::UntapChoice {
                player, candidates, ..
            } => {
                assert_eq!(player, P0);
                assert!(candidates.contains(&seasinger));
                break;
            }
            WaitingFor::Priority { .. } => {
                runner.act(GameAction::PassPriority).expect("pass");
            }
            WaitingFor::DeclareAttackers { .. } => {
                runner
                    .act(GameAction::DeclareAttackers {
                        attacks: vec![],
                        bands: vec![],
                    })
                    .expect("no attack");
            }
            WaitingFor::DeclareBlockers { .. } => {
                runner
                    .act(GameAction::DeclareBlockers {
                        assignments: vec![],
                    })
                    .expect("no block");
            }
            other => panic!("unexpected state before the untap step: {other:?}"),
        }
    }
    let legal = engine::ai_support::legal_actions(runner.state());
    let keep_tapped = GameAction::ChooseUntap {
        object_id: seasinger,
        untap: false,
    };
    assert!(
        legal.contains(&keep_tapped),
        "the decline must be a legal action, got {legal:?}"
    );
    runner.act(keep_tapped).expect("keep Seasinger tapped");
    assert!(runner.state().objects[&seasinger].tapped);
    assert_eq!(
        runner.state().objects[&first].controller,
        P0,
        "controller of {}",
        stringify!(first)
    );
}
