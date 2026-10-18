//! A resolving spell paused on one of its own choices must not move to the
//! graveyard underneath that choice.
//!
//! CR 608.2n: an instant or sorcery is put into its owner's graveyard "as the
//! final part" of its resolution. `resolve_top` used to move it as soon as the
//! effect chain returned, even when the chain had paused on a prompt ("you
//! may …", a library search). With two graveyard-to-exile replacements on the
//! battlefield (two Rest in Peace), that early move asks the CR 616.1 ordering
//! question, and `WaitingFor::ReplacementChoice` overwrote the prompt the
//! resolution was still waiting on:
//! - a "you may" left its `OptionalEffect` frame orphaned on top of the
//!   resolution stack — the game state no longer serialized (save and undo
//!   broke), the question was never asked, and the turn later stalled in
//!   Cleanup, which needs no pending resolution to change phase;
//! - a search was silently dropped.
//!
//! Field report: Dragon's Approach against two Rest in Peace, AI x AI.

use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::types::actions::GameAction;
use engine::types::card_type::CoreType;
use engine::types::game_state::{ReplacementChoiceKind, WaitingFor};
use engine::types::identifiers::ObjectId;
use engine::types::phase::Phase;
use engine::types::zones::Zone;

const DRAGONS_APPROACH: &str = "Dragon's Approach deals 3 damage to each opponent. You may \
exile this spell and four cards named Dragon's Approach from your graveyard. If you do, search \
your library for a Dragon creature card, put it onto the battlefield, then shuffle.";

const REST_IN_PEACE: &str =
    "If a card or token would be put into a graveyard from anywhere, exile it instead.";

const DEMONIC_TUTOR: &str =
    "Search your library for a card, put that card into your hand, then shuffle.";

struct Table {
    runner: GameRunner,
    spell: ObjectId,
    copies: Vec<ObjectId>,
    dragon: ObjectId,
}

fn table(rest_in_peace: usize, copies_in_graveyard: usize, spell_text: &str) -> Table {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    for _ in 0..rest_in_peace {
        scenario.add_enchantment_from_oracle(P1, "Rest in Peace", REST_IN_PEACE);
    }
    let copies = (0..copies_in_graveyard)
        .map(|_| {
            scenario
                .add_spell_to_graveyard(P0, "Dragon's Approach", false)
                .from_oracle_text(DRAGONS_APPROACH)
                .id()
        })
        .collect();
    let dragon = scenario.add_card_to_library_top(P0, "Shivan Dragon");
    let spell = scenario
        .add_spell_to_hand_from_oracle(P0, "Dragon's Approach", false, spell_text)
        .id();
    let mut runner = scenario.build();
    let obj = runner.state_mut().objects.get_mut(&dragon).unwrap();
    obj.card_types.core_types.push(CoreType::Creature);
    obj.card_types.subtypes.push("Dragon".to_string());
    obj.base_card_types = obj.card_types.clone();
    Table {
        runner,
        spell,
        copies,
        dragon,
    }
}

fn zone(runner: &GameRunner, id: ObjectId) -> Zone {
    runner.state().objects[&id].zone
}

/// Cast the spell and pass priority until its resolution asks something.
fn cast_and_resolve(t: &mut Table) {
    let _ = t.runner.cast(t.spell).commit();
    for _ in 0..4 {
        if !matches!(t.runner.state().waiting_for, WaitingFor::Priority { .. }) {
            return;
        }
        t.runner.act(GameAction::PassPriority).unwrap();
    }
}

/// Answer the CR 616.1 ordering prompt for the spell's own move to the
/// graveyard, if one is asked.
fn order_rest_in_peace(t: &mut Table) {
    if let WaitingFor::ReplacementChoice { kind, .. } = &t.runner.state().waiting_for {
        assert_eq!(*kind, ReplacementChoiceKind::Order);
        t.runner
            .act(GameAction::ChooseReplacement { index: 0 })
            .unwrap();
    }
}

/// The resolution is over: priority, no resolution frame left, a state that
/// serializes (save / undo), and a turn that can leave the main phase.
fn assert_settled(t: &mut Table) {
    assert!(
        matches!(t.runner.state().waiting_for, WaitingFor::Priority { .. }),
        "expected priority, got {:?}",
        t.runner.state().waiting_for
    );
    assert!(t.runner.state().stack.is_empty());
    assert_eq!(t.runner.state().resolution_stack.len(), 0);
    serde_json::to_string(t.runner.state()).expect("the game state must serialize");
    t.runner.advance_to_phase(Phase::End);
    assert_eq!(t.runner.state().phase, Phase::End);
}

#[test]
fn two_rest_in_peace_still_ask_the_optional_and_exile_the_spell_last() {
    let mut t = table(2, 0, DRAGONS_APPROACH);
    cast_and_resolve(&mut t);

    // CR 608.2d: the "you may" is asked while the spell is still resolving.
    assert!(
        matches!(
            t.runner.state().waiting_for,
            WaitingFor::OptionalEffectChoice { .. }
        ),
        "the optional must be asked, got {:?}",
        t.runner.state().waiting_for
    );
    assert_eq!(t.runner.life(P1), 17);
    // CR 608.2n: it has not been put anywhere yet.
    assert_eq!(zone(&t.runner, t.spell), Zone::Stack);
    serde_json::to_string(t.runner.state()).expect("the paused state must serialize");

    t.runner
        .act(GameAction::DecideOptionalEffect { accept: false })
        .unwrap();
    // CR 608.2n + CR 616.1: now the spell moves, and the ordering is asked.
    order_rest_in_peace(&mut t);

    assert_eq!(zone(&t.runner, t.spell), Zone::Exile);
    assert_eq!(zone(&t.runner, t.dragon), Zone::Library);
    assert_settled(&mut t);
}

#[test]
fn two_rest_in_peace_with_the_four_copies_pay_the_optional() {
    let mut t = table(2, 4, DRAGONS_APPROACH);
    cast_and_resolve(&mut t);
    assert!(matches!(
        t.runner.state().waiting_for,
        WaitingFor::OptionalEffectChoice { .. }
    ));
    assert_eq!(zone(&t.runner, t.spell), Zone::Stack);

    t.runner
        .act(GameAction::DecideOptionalEffect { accept: true })
        .unwrap();
    for _ in 0..4 {
        match &t.runner.state().waiting_for {
            WaitingFor::SearchChoice { cards, .. } => {
                assert!(cards.contains(&t.dragon));
                let dragon = t.dragon;
                t.runner
                    .act(GameAction::SelectCards {
                        cards: vec![dragon],
                    })
                    .unwrap();
            }
            WaitingFor::ReplacementChoice { .. } => order_rest_in_peace(&mut t),
            _ => break,
        }
    }

    assert_eq!(zone(&t.runner, t.dragon), Zone::Battlefield);
    assert_eq!(zone(&t.runner, t.spell), Zone::Exile);
    for &copy in &t.copies {
        assert_eq!(zone(&t.runner, copy), Zone::Exile);
    }
    assert_settled(&mut t);
}

#[test]
fn one_rest_in_peace_still_asks_the_optional() {
    // No ordering question: the single replacement applies by itself.
    let mut t = table(1, 0, DRAGONS_APPROACH);
    cast_and_resolve(&mut t);
    assert!(matches!(
        t.runner.state().waiting_for,
        WaitingFor::OptionalEffectChoice { .. }
    ));
    t.runner
        .act(GameAction::DecideOptionalEffect { accept: false })
        .unwrap();
    assert_eq!(zone(&t.runner, t.spell), Zone::Exile);
    assert_settled(&mut t);
}

#[test]
fn two_rest_in_peace_do_not_drop_a_pending_search() {
    let mut t = table(2, 0, DEMONIC_TUTOR);
    cast_and_resolve(&mut t);

    let WaitingFor::SearchChoice { cards, .. } = &t.runner.state().waiting_for else {
        panic!(
            "the search must be asked, got {:?}",
            t.runner.state().waiting_for
        );
    };
    assert!(cards.contains(&t.dragon));
    assert_eq!(zone(&t.runner, t.spell), Zone::Stack);
    let dragon = t.dragon;
    t.runner
        .act(GameAction::SelectCards {
            cards: vec![dragon],
        })
        .unwrap();
    order_rest_in_peace(&mut t);

    assert_eq!(zone(&t.runner, t.dragon), Zone::Hand);
    assert_eq!(zone(&t.runner, t.spell), Zone::Exile);
    assert_settled(&mut t);
}
