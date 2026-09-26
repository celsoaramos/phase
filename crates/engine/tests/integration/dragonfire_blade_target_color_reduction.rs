//! Dragonfire Blade — "Equip {4}. This ability costs {1} less to activate for
//! each color of the creature it targets."
//!
//! CR 601.2c + CR 601.2f + CR 602.2b: an activation's targets are chosen before
//! its total cost is determined. The rider counts the colors of the CHOSEN
//! target, so it cannot be folded at announcement, when no target exists: that
//! fold counted zero colors and locked the printed {4}. The equip was then
//! never offered with two or three mana on a two-color creature, and cost four
//! when it was.
//!
//! Legality (CR 118.3) is judged at the best legal target — the activation is
//! legal when some target makes it payable — and the cost actually paid is the
//! one the chosen target sets.

use engine::game::casting::can_activate_ability_now;
use engine::game::game_object::AttachTarget;
use engine::game::scenario::{GameRunner, GameScenario, P0};
use engine::types::ability::TargetRef;
use engine::types::actions::GameAction;
use engine::types::identifiers::ObjectId;
use engine::types::mana::{ManaColor, ManaType, ManaUnit};
use engine::types::phase::Phase;

const DRAGONFIRE_BLADE_ORACLE: &str = "Equipped creature gets +2/+2 and has hexproof from monocolored.\nEquip {4}. This ability costs {1} less to activate for each color of the creature it targets.";

struct Board {
    runner: GameRunner,
    blade: ObjectId,
    boros: ObjectId,
    white: ObjectId,
    colorless: ObjectId,
}

fn board(pool: usize) -> Board {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let boros = scenario
        .add_creature(P0, "Boros Creature", 3, 3)
        .with_color(vec![ManaColor::Red, ManaColor::White])
        .id();
    let white = scenario
        .add_creature(P0, "White Creature", 2, 2)
        .with_color(vec![ManaColor::White])
        .id();
    let colorless = scenario
        .add_creature(P0, "Colorless Creature", 1, 1)
        .with_color(vec![])
        .id();
    let blade = scenario
        .add_creature(P0, "Dragonfire Blade", 0, 1)
        .as_artifact()
        .with_subtypes(vec!["Equipment"])
        .from_oracle_text(DRAGONFIRE_BLADE_ORACLE)
        .id();
    scenario.with_mana_pool(
        P0,
        (0..pool)
            .map(|_| ManaUnit::new(ManaType::Colorless, ObjectId(0), false, vec![]))
            .collect(),
    );
    Board {
        runner: scenario.build(),
        blade,
        boros,
        white,
        colorless,
    }
}

fn pool(runner: &GameRunner) -> usize {
    runner.state().players[0].mana_pool.mana.len()
}

/// Activate the equip, target `host`, and resolve. Returns the mana spent.
fn equip(board: &mut Board, host: ObjectId) -> usize {
    let before = pool(&board.runner);
    board
        .runner
        .act(GameAction::ActivateAbility {
            source_id: board.blade,
            ability_index: 0,
        })
        .expect("the equip is activatable");
    board
        .runner
        .act(GameAction::SelectTargets {
            targets: vec![TargetRef::Object(host)],
        })
        .expect("a creature you control is a legal target");
    let spent = before - pool(&board.runner);
    board.runner.advance_until_stack_empty();
    assert_eq!(
        board.runner.state().objects[&board.blade].attached_to,
        Some(AttachTarget::Object(host)),
        "CR 301.5f: the resolved equip attaches the Blade to the chosen creature"
    );
    spent
}

#[test]
fn two_mana_is_enough_when_a_two_color_creature_can_be_targeted() {
    let b = board(2);
    assert!(
        can_activate_ability_now(b.runner.state(), P0, b.blade, 0),
        "CR 601.2f: targeting the two-color creature makes the equip cost {{2}}"
    );
    let b = board(1);
    assert!(
        !can_activate_ability_now(b.runner.state(), P0, b.blade, 0),
        "no legal target brings the equip below {{2}}"
    );
}

#[test]
fn the_chosen_target_sets_the_cost_paid() {
    let mut b = board(4);
    assert_eq!(
        {
            let host = b.boros;
            equip(&mut b, host)
        },
        2,
        "two colors: {{4}} - 2"
    );

    let mut b = board(4);
    assert_eq!(
        {
            let host = b.white;
            equip(&mut b, host)
        },
        3,
        "one color: {{4}} - 1"
    );

    let mut b = board(4);
    assert_eq!(
        {
            let host = b.colorless;
            equip(&mut b, host)
        },
        4,
        "colorless: the printed {{4}}"
    );
}

#[test]
fn the_reduction_is_applied_once() {
    // Re-equip onto another creature: the second activation is priced by its
    // own target, not by the first one's (or twice).
    let mut b = board(6);
    assert_eq!(
        {
            let host = b.boros;
            equip(&mut b, host)
        },
        2
    );
    assert_eq!(
        {
            let host = b.white;
            equip(&mut b, host)
        },
        3
    );
    assert_eq!(pool(&b.runner), 1);
}
