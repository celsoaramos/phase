//! A remembered "you may" answer must keep a repeat-process loop going on a LATER
//! instance of the same card.
//!
//! MagicFinder table report (03/11/2026): the player answered "Yes, always"
//! (`DecideOptionalEffectAndRemember { Accept, SameCard }`) on the first Cultivator
//! Colossus of the game, which repeated normally. A second Colossus, fetched later in
//! the same turn, put ONE land, drew, and stopped with lands still in hand. Answering
//! "Yes" to every prompt, both Colossi repeat until the lands run out.
//!
//! CR 608.2c: "If you do, draw a card and repeat this process" follows the process
//! again from its start each time a land was put. CR 603.5: a stored answer to the
//! "you may" stands for the player's choice; it must not end the process.

use engine::game::scenario::{GameRunner, GameScenario, P0};
use engine::game::scenario_db::GameScenarioDbExt;
use engine::game::zones::move_to_zone;
use engine::types::actions::GameAction;
use engine::types::card_type::CoreType;
use engine::types::game_state::{AutoMayChoice, MayTriggerAutoChoiceScope, WaitingFor};
use engine::types::identifiers::ObjectId;
use engine::types::mana::{ManaType, ManaUnit};
use engine::types::phase::Phase;
use engine::types::zones::Zone;

use crate::support::shared_card_db;

struct Table {
    runner: GameRunner,
    first: ObjectId,
    second: ObjectId,
    library: Vec<ObjectId>,
}

/// Two Colossi and one Forest in hand; the library is drawn from index 0.
fn table() -> Table {
    let db = shared_card_db().expect("production fixtures must load");
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.add_real_card(P0, "Forest", Zone::Battlefield, db);
    let first = scenario.add_real_card(P0, "Cultivator Colossus", Zone::Hand, db);
    let second = scenario.add_real_card(P0, "Cultivator Colossus", Zone::Hand, db);
    scenario.add_real_card(P0, "Forest", Zone::Hand, db);
    let library = [
        "Shock",
        "Forest",
        "Forest",
        "Forest",
        "Forest",
        "Lightning Bolt",
        "Shock",
    ]
    .iter()
    .map(|name| scenario.add_real_card(P0, name, Zone::Library, db))
    .collect();
    scenario.with_mana_pool(
        P0,
        (0..40)
            .map(|_| ManaUnit::new(ManaType::Green, ObjectId(0), false, vec![]))
            .collect(),
    );
    let mut runner = scenario.build();
    engine::game::rehydrate_game_from_card_db(runner.state_mut(), db);
    Table {
        runner,
        first,
        second,
        library,
    }
}

fn cast_and_resolve_etb(runner: &mut GameRunner, colossus: ObjectId) {
    runner.cast(colossus).commit();
    runner.act(GameAction::PassPriority).expect("caster passes");
    runner
        .act(GameAction::PassPriority)
        .expect("opponent passes; the Colossus resolves");
    assert_eq!(runner.state().objects[&colossus].zone, Zone::Battlefield);
    runner.resolve_top();
}

/// Answer every prompt of the process: `first_answer` for the first "you may", plain
/// accepts after that, and the first offered land at each card choice.
fn drive(runner: &mut GameRunner, mut first_answer: Option<GameAction>) -> usize {
    let mut prompts = 0;
    for _ in 0..80 {
        let action = match &runner.state().waiting_for {
            WaitingFor::OptionalEffectChoice { .. } => {
                prompts += 1;
                first_answer
                    .take()
                    .unwrap_or(GameAction::DecideOptionalEffect { accept: true })
            }
            WaitingFor::EffectZoneChoice { cards, .. } => GameAction::SelectCards {
                cards: vec![cards[0]],
            },
            WaitingFor::Priority { .. } => return prompts,
            other => panic!("unexpected prompt: {other:?}"),
        };
        runner.act(action).expect("the answer is accepted");
    }
    panic!("the process did not return to priority");
}

fn lands_on_battlefield(runner: &GameRunner) -> usize {
    let st = runner.state();
    st.battlefield
        .iter()
        .filter(|id| {
            st.objects[id]
                .card_types
                .core_types
                .contains(&CoreType::Land)
        })
        .count()
}

/// Runs the first Colossus with `first_answer`, refills the hand with two Forests, then
/// resolves the second Colossus and returns how many lands IT put.
fn second_colossus_puts(first_answer: GameAction) -> usize {
    let Table {
        mut runner,
        first,
        second,
        library,
    } = table();
    cast_and_resolve_etb(&mut runner, first);
    drive(&mut runner, Some(first_answer));
    // First Colossus: puts the hand Forest, draws Shock, and the next pass has no land
    // to put — that infeasible pass is what used to leave a stale payment failure.
    assert_eq!(lands_on_battlefield(&runner), 2);
    for id in [library[1], library[2]] {
        move_to_zone(runner.state_mut(), id, Zone::Hand, &mut vec![]);
    }
    let before = lands_on_battlefield(&runner);
    let draws = runner.state().players[0].cards_drawn_this_turn;
    cast_and_resolve_etb(&mut runner, second);
    drive(&mut runner, None);
    let drew = (runner.state().players[0].cards_drawn_this_turn - draws) as usize;
    let put = lands_on_battlefield(&runner) - before;
    assert_eq!(drew, put, "every land put this way draws a card");
    put
}

#[test]
fn second_colossus_repeats_when_each_prompt_is_answered() {
    // Two Forests in hand, then drawn Forest, Forest, Lightning Bolt, Shock: four puts.
    assert_eq!(
        second_colossus_puts(GameAction::DecideOptionalEffect { accept: true }),
        4
    );
}

#[test]
fn second_colossus_repeats_after_a_same_card_remembered_accept() {
    let remembered = GameAction::DecideOptionalEffectAndRemember {
        choice: AutoMayChoice::Accept,
        scope: MayTriggerAutoChoiceScope::SameCard,
    };
    assert_eq!(second_colossus_puts(remembered), 4);
}
