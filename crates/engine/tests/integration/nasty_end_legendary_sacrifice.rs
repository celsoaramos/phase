//! CR 205.4a + CR 608.2c + CR 608.2h: Nasty End — "As an additional cost to
//! cast this spell, sacrifice a creature. Draw two cards. If the sacrificed
//! creature was legendary, draw three cards instead."
//!
//! The sacrificed creature is gone by resolution, so the legendary check reads
//! the cost-paid LKI. A nonlegendary sacrifice must still draw two.

use engine::game::scenario::GameScenario;
use engine::types::actions::GameAction;
use engine::types::game_state::{CastPaymentMode, WaitingFor};
use engine::types::mana::{ManaCost, ManaCostShard, ManaType, ManaUnit};
use engine::types::phase::Phase;
use engine::types::player::PlayerId;
use engine::types::ObjectId;

const P0: PlayerId = PlayerId(0);

const NASTY_END: &str = "As an additional cost to cast this spell, sacrifice a creature.\n\
Draw two cards. If the sacrificed creature was legendary, draw three cards instead.";

fn run(legendary_victim: bool) -> usize {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    for i in 0..4 {
        scenario.add_card_to_library_top(P0, &format!("Library {i}"));
    }
    scenario.with_mana_pool(
        P0,
        vec![ManaUnit::new(ManaType::Black, ObjectId(0), false, vec![])],
    );
    let spell = scenario
        .add_spell_to_hand_from_oracle(P0, "Nasty End", true, NASTY_END)
        .with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::Black],
            generic: 0,
        })
        .id();
    let victim = {
        let mut builder = scenario.add_creature(P0, "Victim", 1, 1);
        if legendary_victim {
            builder.as_legendary();
        }
        builder.id()
    };
    let mut runner = scenario.build();
    let card_id = runner.state().objects[&spell].card_id;
    runner
        .act(GameAction::CastSpell {
            object_id: spell,
            card_id,
            targets: vec![],
            payment_mode: CastPaymentMode::Auto,
        })
        .expect("cast Nasty End");

    let hand_after_cast =
        |r: &engine::game::scenario::GameRunner| r.state().players[P0.0 as usize].hand.len();
    let mut baseline: Option<usize> = None;
    for _ in 0..32 {
        match &runner.state().waiting_for {
            WaitingFor::ManaPayment { .. } => {
                runner.act(GameAction::PassPriority).expect("finalize mana");
            }
            WaitingFor::PayCost { .. } => {
                runner
                    .act(GameAction::SelectCards {
                        cards: vec![victim],
                    })
                    .expect("sacrifice the victim");
            }
            WaitingFor::Priority { .. } => {
                let stack_empty = runner.state().stack.is_empty();
                if !stack_empty && baseline.is_none() {
                    baseline = Some(hand_after_cast(&runner));
                }
                if stack_empty && baseline.is_some() {
                    break;
                }
                runner.act(GameAction::PassPriority).expect("pass priority");
            }
            other => panic!("unexpected window: {other:?}"),
        }
    }
    assert!(
        !runner.state().battlefield.contains(&victim),
        "the victim was sacrificed as the additional cost"
    );
    hand_after_cast(&runner) - baseline.expect("spell reached the stack")
}

#[test]
fn nasty_end_draws_three_for_a_legendary_sacrifice() {
    assert_eq!(run(false), 2, "nonlegendary sacrifice draws two");
    assert_eq!(
        run(true),
        3,
        "CR 608.2c: a legendary sacrifice draws three instead"
    );
}
