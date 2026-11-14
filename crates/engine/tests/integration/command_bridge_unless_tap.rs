//! Runtime regression for Command Bridge (Oracle text: "This land enters
//! tapped.\nWhen this land enters, sacrifice it unless you tap an untapped
//! permanent you control.\n{T}: Add one mana of any color.").
//!
//! Reported bug: Command Bridge was ALWAYS sacrificed, even with untapped
//! permanents on the battlefield. The parser produced the right unless-cost
//! (`AbilityCost::TapCreatures` over "untapped permanent you control") and the
//! engine offered `PayUnlessCost { pay: true }`, but `handle_unless_payment`
//! (`engine_payment_choices.rs`) routed `TapCreatures` to the "no unless-payment
//! dialog" fall-through: paying silently failed and the sacrifice happened.
//!
//! The fix surfaces the same `PayCost { TapCreatures }` object choice that the
//! resolution-time "you may tap" costs use, resumed by `CostResume::
//! UnlessPayment` so the completed selection settles the unless payment (the
//! guarded sacrifice does not happen).
//!
//! CR ANCHORS (verified against docs/MagicCompRules.txt):
//!   * CR 118.12 — "[Do something] unless [a player does something else]."
//!   * CR 701.26a — to tap a permanent, turn it sideways.
//!   * CR 603.6a — a "when it enters" trigger fires when the permanent enters.

use engine::game::scenario::{GameRunner, GameScenario, P0};
use engine::types::actions::GameAction;
use engine::types::game_state::{CostResume, WaitingFor};
use engine::types::identifiers::ObjectId;
use engine::types::mana::ManaColor;
use engine::types::phase::Phase;
use engine::types::zones::Zone;

const COMMAND_BRIDGE: &str = "This land enters tapped.\nWhen this land enters, sacrifice \
    it unless you tap an untapped permanent you control.\n{T}: Add one mana of any color.";

struct Board {
    runner: GameRunner,
    bridge: ObjectId,
    plains: Vec<ObjectId>,
}

/// Play Command Bridge with `untapped_plains` untapped Plains already on the
/// battlefield and run its ETB trigger up to the `UnlessPayment` decision.
fn play_bridge_and_reach_payment_prompt(untapped_plains: usize) -> Board {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let plains: Vec<ObjectId> = (0..untapped_plains)
        .map(|_| scenario.add_basic_land(P0, ManaColor::White))
        .collect();
    let bridge = scenario
        .add_land_to_hand(P0, "Command Bridge")
        .from_oracle_text(COMMAND_BRIDGE)
        .id();

    let mut runner = scenario.build();
    let card_id = runner.state().objects[&bridge].card_id;
    runner
        .act(GameAction::PlayLand {
            object_id: bridge,
            card_id,
        })
        .expect("playing Command Bridge must succeed");
    assert!(
        runner.state().objects[&bridge].tapped,
        "Command Bridge enters tapped"
    );
    runner.resolve_top();

    assert!(
        matches!(
            runner.state().waiting_for,
            WaitingFor::UnlessPayment { player, .. } if player == P0
        ),
        "Command Bridge's ETB must offer the unless-payment to its controller, got {:?}",
        runner.state().waiting_for
    );
    Board {
        runner,
        bridge,
        plains,
    }
}

/// CR 118.12 + CR 701.26a: choosing to pay asks WHICH untapped permanent to
/// tap; tapping it pays the cost and Command Bridge stays. This is the
/// reported bug — before the fix, paying sacrificed the land anyway and no
/// permanent became tapped.
#[test]
fn command_bridge_stays_when_controller_taps_an_untapped_permanent() {
    let mut board = play_bridge_and_reach_payment_prompt(2);

    board
        .runner
        .act(GameAction::PayUnlessCost { pay: true })
        .expect("the controller may choose to pay");

    match &board.runner.state().waiting_for {
        WaitingFor::PayCost {
            player,
            choices,
            count,
            min_count,
            resume: CostResume::UnlessPayment { .. },
            ..
        } => {
            assert_eq!(*player, P0);
            assert_eq!((*min_count, *count), (1, 1), "exactly one permanent");
            for plains in &board.plains {
                assert!(
                    choices.contains(plains),
                    "every untapped permanent is eligible, got {choices:?}"
                );
            }
            assert!(
                !choices.contains(&board.bridge),
                "Command Bridge itself is tapped and can't pay, got {choices:?}"
            );
        }
        other => panic!("expected a PayCost tap selection, got {other:?}"),
    }

    let tapped = board.plains[1];
    board
        .runner
        .act(GameAction::SelectCards {
            cards: vec![tapped],
        })
        .expect("tapping an eligible permanent pays the cost");

    let state = board.runner.state();
    assert_eq!(
        state.objects[&board.bridge].zone,
        Zone::Battlefield,
        "a paid unless-cost suppresses the sacrifice"
    );
    assert!(
        state.objects[&tapped].tapped,
        "the chosen permanent is tapped"
    );
    assert!(
        !state.objects[&board.plains[0]].tapped,
        "only the chosen permanent is tapped"
    );
    assert!(
        matches!(state.waiting_for, WaitingFor::Priority { .. }),
        "the game returns to priority, got {:?}",
        state.waiting_for
    );
    assert!(state.stack.is_empty(), "the trigger has resolved");
}

/// CR 118.12: a selection outside the eligible set is refused and the prompt
/// stays open — paying can't tap the already-tapped Command Bridge.
#[test]
fn command_bridge_refuses_tapping_an_ineligible_permanent() {
    let mut board = play_bridge_and_reach_payment_prompt(1);
    board
        .runner
        .act(GameAction::PayUnlessCost { pay: true })
        .expect("the controller may choose to pay");
    let bridge = board.bridge;
    assert!(board
        .runner
        .act(GameAction::SelectCards {
            cards: vec![bridge],
        })
        .is_err());
    assert!(matches!(
        board.runner.state().waiting_for,
        WaitingFor::PayCost { .. }
    ));
}

/// CR 118.12: declining still sacrifices Command Bridge (control group).
#[test]
fn command_bridge_is_sacrificed_when_controller_declines() {
    let mut board = play_bridge_and_reach_payment_prompt(2);
    board
        .runner
        .act(GameAction::PayUnlessCost { pay: false })
        .expect("declining is always legal");
    let state = board.runner.state();
    assert_eq!(state.objects[&board.bridge].zone, Zone::Graveyard);
    assert!(board.plains.iter().all(|id| !state.objects[id].tapped));
}

/// CR 118.12 + CR 118.3: with no other untapped permanent the cost can't be
/// paid, so choosing to pay falls through to the sacrifice.
#[test]
fn command_bridge_is_sacrificed_without_an_untapped_permanent() {
    let mut board = play_bridge_and_reach_payment_prompt(0);
    board
        .runner
        .act(GameAction::PayUnlessCost { pay: true })
        .expect("choosing to pay an unpayable cost is answered, not refused");
    assert_eq!(
        board.runner.state().objects[&board.bridge].zone,
        Zone::Graveyard
    );
}
