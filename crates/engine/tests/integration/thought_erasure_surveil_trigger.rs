//! Thought Erasure: "Target opponent reveals their hand. You choose a nonland
//! card from it. That player discards that card. Surveil 1."
//!
//! Reported from play: "whenever you surveil" creatures (Dimir Spybug) did not
//! get their +1/+1 counter when the surveil came after the hand choice.

use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::types::ability::TargetRef;
use engine::types::actions::GameAction;
use engine::types::counter::CounterType;
use engine::types::game_state::{CastPaymentMode, WaitingFor};
use engine::types::identifiers::ObjectId;
use engine::types::mana::{ManaCost, ManaCostShard, ManaType, ManaUnit};
use engine::types::phase::Phase;

const THOUGHT_ERASURE: &str = "Target opponent reveals their hand. You choose a nonland card from it. That player discards that card.\nSurveil 1.";
const PLAIN_SURVEIL: &str = "Surveil 1.";
const MEGRIM: &str =
    "Whenever an opponent discards a card, this enchantment deals 2 damage to that player.";
const DIMIR_SPYBUG: &str =
    "Flying\nMenace\nWhenever you surveil, put a +1/+1 counter on this creature.";

fn counters(r: &GameRunner, id: ObjectId) -> u32 {
    *r.state().objects[&id]
        .counters
        .get(&CounterType::Plus1Plus1)
        .unwrap_or(&0)
}

/// Cast `spell` and answer every prompt until the stack is empty. Records the
/// prompts seen. `keep_on_top` keeps the surveilled card on top (else it is milled).
fn cast_and_settle(r: &mut GameRunner, spell: ObjectId, keep_on_top: bool) -> Vec<&'static str> {
    let card_id = r.state().objects[&spell].card_id;
    r.act(GameAction::CastSpell {
        object_id: spell,
        card_id,
        targets: vec![],
        payment_mode: CastPaymentMode::Auto,
    })
    .expect("cast");
    let mut seen = Vec::new();
    for _ in 0..60 {
        let kind = r.waiting_for_kind();
        let action = match r.state().waiting_for.clone() {
            WaitingFor::TargetSelection { .. } | WaitingFor::TriggerTargetSelection { .. } => {
                GameAction::ChooseTarget {
                    target: Some(TargetRef::Player(P1)),
                }
            }
            WaitingFor::RevealChoice { cards, .. } => GameAction::SelectCards {
                cards: cards.into_iter().take(1).collect(),
            },
            WaitingFor::SurveilChoice { cards, .. } => GameAction::SelectCards {
                cards: if keep_on_top { cards } else { vec![] },
            },
            WaitingFor::OrderTriggers { triggers, .. } => GameAction::OrderTriggers {
                order: (0..triggers.len()).collect(),
            },
            WaitingFor::ManaPayment { .. } => GameAction::PassPriority,
            WaitingFor::Priority { .. } if r.state().stack.is_empty() => break,
            _ => GameAction::PassPriority,
        };
        seen.push(kind);
        r.act(action).expect("the spell proceeds");
    }
    seen
}

fn board(oracle: &str, name: &str) -> (GameRunner, ObjectId, ObjectId) {
    board_with(oracle, name, false)
}

fn board_with(oracle: &str, name: &str, megrim: bool) -> (GameRunner, ObjectId, ObjectId) {
    let mut sc = GameScenario::new();
    if megrim {
        sc.add_enchantment_from_oracle(P0, "Megrim", MEGRIM);
    }
    sc.at_phase(Phase::PreCombatMain);
    for _ in 0..5 {
        sc.add_card_to_library_top(P0, "Library Card");
    }
    let spybug = sc
        .add_creature_from_oracle(P0, "Dimir Spybug", 1, 1, DIMIR_SPYBUG)
        .id();
    sc.add_creature_to_hand(P1, "Opponent Creature", 2, 2);
    let spell = sc
        .add_spell_to_hand_from_oracle(P0, name, false, oracle)
        .with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::Blue, ManaCostShard::Black],
            generic: 0,
        })
        .id();
    sc.with_mana_pool(
        P0,
        vec![
            ManaUnit::new(ManaType::Blue, ObjectId(0), false, vec![]),
            ManaUnit::new(ManaType::Black, ObjectId(0), false, vec![]),
        ],
    );
    (sc.build(), spybug, spell)
}

/// Control: a bare "Surveil 1." grows the Spybug.
#[test]
fn plain_surveil_grows_dimir_spybug() {
    let (mut r, spybug, spell) = board(PLAIN_SURVEIL, "Plain Surveil");
    let seen = cast_and_settle(&mut r, spell, false);
    assert!(seen.contains(&"SurveilChoice"), "prompts: {seen:?}");
    assert_eq!(counters(&r, spybug), 1, "prompts: {seen:?}");
}

/// CR 701.25a + CR 603.2: the surveil at the end of Thought Erasure is an
/// instructed surveil like any other — "whenever you surveil" triggers.
#[test]
fn thought_erasure_surveil_grows_dimir_spybug() {
    for keep_on_top in [false, true] {
        let (mut r, spybug, spell) = board(THOUGHT_ERASURE, "Thought Erasure");
        let seen = cast_and_settle(&mut r, spell, keep_on_top);
        assert!(seen.contains(&"RevealChoice"), "prompts: {seen:?}");
        assert!(seen.contains(&"SurveilChoice"), "prompts: {seen:?}");
        assert_eq!(
            r.state().players[1].hand.len(),
            0,
            "the opponent discarded the chosen card"
        );
        assert_eq!(
            counters(&r, spybug),
            1,
            "keep_on_top={keep_on_top}, prompts: {seen:?}"
        );
    }
}

/// CR 603.2 + CR 603.3b: the discard that ran before the surveil pause is an
/// observable event too — "whenever an opponent discards" triggers exactly once,
/// alongside the surveil trigger, once the spell finishes resolving.
#[test]
fn thought_erasure_discard_and_surveil_observers_trigger_once() {
    let (mut r, spybug, spell) = board_with(THOUGHT_ERASURE, "Thought Erasure", true);
    let life_before = r.state().players[1].life;
    let seen = cast_and_settle(&mut r, spell, false);
    assert!(seen.contains(&"SurveilChoice"), "prompts: {seen:?}");
    assert_eq!(counters(&r, spybug), 1, "prompts: {seen:?}");
    assert_eq!(
        r.state().players[1].life,
        life_before - 2,
        "Megrim triggers once for the discard; prompts: {seen:?}"
    );
}
