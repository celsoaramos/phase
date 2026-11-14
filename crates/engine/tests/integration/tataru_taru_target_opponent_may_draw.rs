//! CR 115.1 + CR 608.2d: Tataru Taru — "When Tataru Taru enters, you draw a
//! card and target opponent may draw a card."
//!
//! The opponent is TARGETED (chosen as the trigger goes on the stack) and that
//! same opponent announces the "may" — not the controller and not every
//! opponent. Three players so a per-opponent fan-out would be visible.

use engine::game::scenario::GameScenario;
use engine::types::ability::TargetRef;
use engine::types::actions::GameAction;
use engine::types::game_state::{CastPaymentMode, WaitingFor};
use engine::types::mana::{ManaCost, ManaCostShard, ManaType, ManaUnit};
use engine::types::phase::Phase;
use engine::types::player::PlayerId;

const TATARU: &str =
    "When Tataru Taru enters, you draw a card and target opponent may draw a card.";

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const P2: PlayerId = PlayerId(2);

/// Returns (cards drawn by P0, P1, P2, players asked the optional).
fn run(accept: bool) -> (usize, usize, usize, Vec<PlayerId>) {
    let mut scenario = GameScenario::new_n_player(3, 7);
    scenario.at_phase(Phase::PreCombatMain);
    let tataru = scenario
        .add_creature_to_hand_from_oracle(P0, "Tataru Taru", 0, 3, TATARU)
        .with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::White],
            generic: 0,
        })
        .id();
    for p in [P0, P1, P2] {
        for _ in 0..3 {
            scenario.add_card_to_library_top(p, "Plains");
        }
    }
    scenario.with_mana_pool(
        P0,
        vec![ManaUnit::new(ManaType::White, tataru, false, Vec::new())],
    );
    let mut runner = scenario.build();
    let library = |r: &engine::game::scenario::GameRunner, p: PlayerId| {
        r.state().players[p.0 as usize].library.len()
    };
    let before = [
        library(&runner, P0),
        library(&runner, P1),
        library(&runner, P2),
    ];

    let card_id = runner.state().objects[&tataru].card_id;
    runner
        .act(GameAction::CastSpell {
            object_id: tataru,
            card_id,
            targets: vec![],
            payment_mode: CastPaymentMode::Auto,
        })
        .expect("cast Tataru Taru");

    let mut targeted = false;
    let mut asked = Vec::new();
    for _ in 0..60 {
        match runner.state().waiting_for.clone() {
            WaitingFor::ManaPayment { .. } => {
                runner.act(GameAction::PassPriority).expect("pay mana");
            }
            WaitingFor::TriggerTargetSelection {
                target_slots,
                selection,
                ..
            } => {
                let slot = &target_slots[selection.current_slot];
                assert!(
                    slot.legal_targets.contains(&TargetRef::Player(P2))
                        && !slot.legal_targets.contains(&TargetRef::Player(P0)),
                    "the trigger targets an opponent; got {:?}",
                    slot.legal_targets
                );
                runner
                    .act(GameAction::ChooseTarget {
                        target: Some(TargetRef::Player(P2)),
                    })
                    .expect("target P2");
                targeted = true;
            }
            WaitingFor::OptionalEffectChoice { player, .. } => {
                asked.push(player);
                runner
                    .act(GameAction::DecideOptionalEffect { accept })
                    .expect("answer the optional draw");
            }
            WaitingFor::OrderTriggers { .. } => {
                runner.advance_until_stack_empty();
            }
            WaitingFor::Priority { .. } => {
                if runner.state().stack.is_empty() && targeted {
                    break;
                }
                runner.act(GameAction::PassPriority).expect("pass priority");
            }
            other => panic!("unexpected prompt: {other:?}"),
        }
    }
    assert!(targeted, "the ETB must ask for a target opponent");
    (
        before[0] - library(&runner, P0),
        before[1] - library(&runner, P1),
        before[2] - library(&runner, P2),
        asked,
    )
}

#[test]
fn tataru_taru_targeted_opponent_decides_and_draws() {
    let (p0, p1, p2, asked) = run(true);
    assert_eq!(p0, 1, "the controller draws a card");
    assert_eq!(p2, 1, "the targeted opponent drew after accepting");
    assert_eq!(p1, 0, "an untargeted opponent draws nothing");
    assert_eq!(
        asked,
        vec![P2],
        "CR 608.2d: only the targeted opponent announces the may"
    );
}

#[test]
fn tataru_taru_targeted_opponent_may_decline() {
    let (p0, p1, p2, asked) = run(false);
    assert_eq!(p0, 1, "the controller still draws");
    assert_eq!((p1, p2), (0, 0), "a declined may draws nothing");
    assert_eq!(asked, vec![P2]);
}
