//! CR 601.2c + CR 603.2 + CR 603.3b — a spell's target announcement fires
//! "becomes the target of a spell" triggers even when the cast pauses before
//! the spell reaches the stack.
//!
//! Field report (Venerated Rotpriest + Apostle's Blessing): the Blessing's
//! `{W/P}` paid with life pauses the cast at `WaitingFor::PhyrexianPayment`.
//! The `BecomesTarget` events were produced by the action that paused, and the
//! post-action trigger scan only runs on actions that settle at `Priority`, so
//! the events were dropped and the opponent never got the poison counter. A
//! manual mana payment (`WaitingFor::ManaPayment`) lost them the same way, and
//! so did an optional/alternative cost prompt (`WaitingFor::OptionalCostChoice`,
//! Invigorate's "an opponent gains 3 life" — its `PendingCast` rides inside the
//! prompt, not on `GameState`); only
//! an auto-paid cast, which reaches the stack in the announcing action, fired.

use engine::game::scenario::{GameScenario, GameRunner, P0, P1};
use engine::types::ability::TargetRef;
use engine::types::actions::GameAction;
use engine::types::game_state::{CastPaymentMode, ShardChoice, WaitingFor};
use engine::types::identifiers::ObjectId;
use engine::types::mana::{ManaCost, ManaCostShard, ManaType, ManaUnit};
use engine::types::phase::Phase;

const ROTPRIEST: &str =
    "Whenever a creature you control becomes the target of a spell, target opponent gets a poison counter.";
const PUMP: &str = "Target creature you control gets +1/+1 until end of turn.";

fn setup(cost: ManaCost, pool: usize) -> (GameRunner, ObjectId, ObjectId) {
    setup_with(PUMP, cost, pool)
}

fn setup_with(pump_oracle: &str, cost: ManaCost, pool: usize) -> (GameRunner, ObjectId, ObjectId) {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let priest = scenario
        .add_creature_from_oracle(P0, "Venerated Rotpriest", 1, 2, ROTPRIEST)
        .id();
    let pump = scenario
        .add_spell_to_hand_from_oracle(P0, "Test Pump", true, pump_oracle)
        .id();
    scenario.with_mana_pool(
        P0,
        (0..pool)
            .map(|_| ManaUnit::new(ManaType::Colorless, ObjectId(0), false, vec![]))
            .collect(),
    );
    let mut runner = scenario.build();
    runner.state_mut().objects.get_mut(&pump).unwrap().mana_cost = cost;
    (runner, priest, pump)
}

fn announce(runner: &mut GameRunner, pump: ObjectId, priest: ObjectId, mode: CastPaymentMode) {
    let card_id = runner.state().objects[&pump].card_id;
    runner
        .act(GameAction::CastSpell {
            object_id: pump,
            card_id,
            targets: vec![],
            payment_mode: mode,
        })
        .expect("announce the pump spell");
    if matches!(runner.state().waiting_for, WaitingFor::TargetSelection { .. }) {
        runner
            .act(GameAction::SelectTargets {
                targets: vec![TargetRef::Object(priest)],
            })
            .expect("target the Rotpriest");
    }
}

fn resolve_all(runner: &mut GameRunner) {
    for _ in 0..8 {
        if runner.state().stack.is_empty() {
            break;
        }
        runner.act(GameAction::PassPriority).expect("pass priority");
    }
    assert!(runner.state().stack.is_empty(), "stack should have resolved");
}

fn poison(runner: &GameRunner) -> u32 {
    runner.state().players[P1.0 as usize].poison_counters
}

fn phyrexian_white() -> ManaCost {
    ManaCost::Cost {
        generic: 0,
        shards: vec![ManaCostShard::PhyrexianWhite],
    }
}

#[test]
fn phyrexian_life_payment_keeps_becomes_target_trigger() {
    let (mut runner, priest, pump) = setup(phyrexian_white(), 0);
    announce(&mut runner, pump, priest, CastPaymentMode::Auto);
    assert!(matches!(
        runner.state().waiting_for,
        WaitingFor::PhyrexianPayment { .. }
    ));
    runner
        .act(GameAction::SubmitPhyrexianChoices {
            choices: vec![ShardChoice::PayLife],
        })
        .expect("pay {W/P} with life");
    assert_eq!(
        runner.state().stack.len(),
        2,
        "the Rotpriest trigger goes on the stack above the spell"
    );
    resolve_all(&mut runner);
    assert_eq!(poison(&runner), 1);
}

#[test]
fn manual_mana_payment_keeps_becomes_target_trigger() {
    let (mut runner, priest, pump) = setup(
        ManaCost::Cost {
            generic: 1,
            shards: vec![],
        },
        1,
    );
    announce(&mut runner, pump, priest, CastPaymentMode::Manual);
    assert!(matches!(
        runner.state().waiting_for,
        WaitingFor::ManaPayment { .. }
    ));
    runner
        .act(GameAction::PassPriority)
        .expect("confirm the manual payment");
    assert_eq!(runner.state().stack.len(), 2);
    resolve_all(&mut runner);
    assert_eq!(poison(&runner), 1);
}

#[test]
fn auto_paid_cast_still_fires_exactly_once() {
    let (mut runner, priest, pump) = setup(
        ManaCost::Cost {
            generic: 1,
            shards: vec![],
        },
        1,
    );
    announce(&mut runner, pump, priest, CastPaymentMode::Auto);
    assert_eq!(runner.state().stack.len(), 2);
    resolve_all(&mut runner);
    assert_eq!(poison(&runner), 1);
}

#[test]
fn cancelled_cast_fires_nothing() {
    let (mut runner, priest, pump) = setup(phyrexian_white(), 0);
    announce(&mut runner, pump, priest, CastPaymentMode::Auto);
    runner
        .act(GameAction::CancelCast)
        .expect("cancel during the Phyrexian choice");
    assert!(runner.state().stack.is_empty());
    assert!(runner.state().held_cast_target_events.is_none());
    // The next cast of the same card fires once — nothing left over from the
    // cancelled announcement.
    announce(&mut runner, pump, priest, CastPaymentMode::Auto);
    runner
        .act(GameAction::SubmitPhyrexianChoices {
            choices: vec![ShardChoice::PayLife],
        })
        .expect("pay {W/P} with life");
    resolve_all(&mut runner);
    assert_eq!(poison(&runner), 1);
}

/// The optional-cost prompt embeds its `PendingCast` in the `WaitingFor`
/// (`pending_cast_ref`) rather than in `GameState::pending_cast`. Buyback asks
/// after the targets are announced, the same seam as Invigorate's
/// alternative cost.
#[test]
fn optional_cost_prompt_keeps_becomes_target_trigger() {
    let oracle = format!("Buyback {{2}}\n{PUMP}");
    let (mut runner, priest, pump) = setup_with(
        &oracle,
        ManaCost::Cost {
            generic: 1,
            shards: vec![],
        },
        // Enough to afford the buyback, or the prompt is not offered.
        3,
    );
    announce(&mut runner, pump, priest, CastPaymentMode::Auto);
    assert!(
        matches!(
            runner.state().waiting_for,
            WaitingFor::OptionalCostChoice { .. }
        ),
        "expected the buyback prompt, got {:?}",
        runner.state().waiting_for
    );
    runner
        .act(GameAction::DecideOptionalCost { pay: false })
        .expect("decline buyback");
    assert_eq!(runner.state().stack.len(), 2);
    resolve_all(&mut runner);
    assert_eq!(poison(&runner), 1);
}
