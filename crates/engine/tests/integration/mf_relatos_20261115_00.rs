//! Field report of 2026-11-14 (MagicFinder mesa, engine v0.125.0):
//!
//! * Winds of Abandon, overloaded — "Exile each creature you don't control. For
//!   each creature exiled this way, its controller searches their library for a
//!   basic land card. Those players put those cards onto the battlefield tapped,
//!   then shuffle." The caster searched their own library and BOTH libraries were
//!   put onto the battlefield in full (creatures, artifacts, nonbasic lands).

use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::game::scenario_db::GameScenarioDbExt;
use engine::types::actions::{AlternativeCastDecision, GameAction};
use engine::types::game_state::{AlternativeCastKeyword, CastPaymentMode, WaitingFor};
use engine::types::identifiers::ObjectId;
use engine::types::mana::{ManaType, ManaUnit};
use engine::types::phase::Phase;
use engine::types::player::PlayerId;
use engine::types::zones::Zone;

use crate::support::shared_card_db;

fn zone_names<'a>(runner: &GameRunner, ids: impl IntoIterator<Item = &'a ObjectId>) -> Vec<String> {
    let mut names: Vec<String> = ids
        .into_iter()
        .map(|id| runner.state().objects[id].name.clone())
        .collect();
    names.sort();
    names
}

fn player(runner: &GameRunner, id: PlayerId) -> &engine::types::player::Player {
    runner.state().players.iter().find(|p| p.id == id).unwrap()
}

fn battlefield_names(runner: &GameRunner, controller: PlayerId) -> Vec<String> {
    let ids: Vec<ObjectId> = runner
        .state()
        .battlefield
        .iter()
        .copied()
        .filter(|id| runner.state().objects[id].controller == controller)
        .collect();
    zone_names(runner, &ids)
}

/// Cast with overload and settle: every search prompt takes the first basic
/// land offered; every other prompt passes priority.
fn cast_overloaded_and_settle(runner: &mut GameRunner, spell: ObjectId) -> Vec<PlayerId> {
    let card_id = runner.state().objects[&spell].card_id;
    runner
        .act(GameAction::CastSpell {
            object_id: spell,
            card_id,
            targets: vec![],
            payment_mode: CastPaymentMode::Auto,
        })
        .expect("cast Winds of Abandon");
    assert!(
        matches!(
            runner.state().waiting_for,
            WaitingFor::AlternativeCastChoice {
                keyword: AlternativeCastKeyword::Overload,
                ..
            }
        ),
        "expected the overload choice, got {:?}",
        runner.state().waiting_for
    );
    runner
        .act(GameAction::ChooseAlternativeCast {
            choice: AlternativeCastDecision::Alternative,
        })
        .expect("opt into overload");

    let mut searchers = Vec::new();
    for _ in 0..60 {
        let search = match &runner.state().waiting_for {
            WaitingFor::SearchChoice { player, cards, .. } => Some((*player, cards.clone())),
            _ => None,
        };
        if let Some((who, cards)) = search {
            searchers.push(who);
            for id in &cards {
                let obj = &runner.state().objects[id];
                assert_eq!(obj.owner, who, "a searcher looks at their own library");
            }
            let pick: Vec<ObjectId> = cards.into_iter().take(1).collect();
            runner
                .act(GameAction::SelectCards { cards: pick })
                .expect("the searcher takes a basic land");
            continue;
        }
        if runner.state().stack.is_empty()
            && matches!(runner.state().waiting_for, WaitingFor::Priority { .. })
        {
            break;
        }
        if runner.act(GameAction::PassPriority).is_err() {
            break;
        }
    }
    searchers
}

#[test]
fn winds_of_abandon_overload_fetches_one_basic_per_exiled_creature() {
    let Some(db) = shared_card_db() else {
        eprintln!("skipping: integration card fixture not available");
        return;
    };

    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let winds = scenario.add_real_card(P0, "Winds of Abandon", Zone::Hand, db);
    let own_creature = scenario.add_creature(P0, "Grizzly Bears", 2, 2).id();
    let theirs_a = scenario.add_creature(P1, "Hill Giant", 3, 3).id();
    let theirs_b = scenario.add_creature(P1, "Gray Ogre", 2, 2).id();
    // The caster's library: nothing here may move.
    scenario.add_real_card(P0, "Forest", Zone::Library, db);
    scenario.add_real_card(P0, "Shivan Dragon", Zone::Library, db);
    scenario.add_real_card(P0, "Sol Ring", Zone::Library, db);
    // The opponent's library: three basics (two exiled creatures fetch two) and
    // cards a basic-land search can never put onto the battlefield.
    scenario.add_real_card(P1, "Plains", Zone::Library, db);
    scenario.add_real_card(P1, "Shivan Dragon", Zone::Library, db);
    scenario.add_real_card(P1, "Swamp", Zone::Library, db);
    scenario.add_real_card(P1, "Sol Ring", Zone::Library, db);
    scenario.add_real_card(P1, "Mountain", Zone::Library, db);
    scenario.add_real_card(P1, "Command Tower", Zone::Library, db);
    scenario.with_mana_pool(
        P0,
        (0..6)
            .map(|_| ManaUnit::new(ManaType::White, ObjectId(0), false, vec![]))
            .collect(),
    );
    let mut runner = scenario.build();
    engine::game::rehydrate_game_from_card_db(runner.state_mut(), db);

    let searchers = cast_overloaded_and_settle(&mut runner, winds);

    // CR 702.96b: each creature the caster doesn't control is exiled; theirs stays.
    for id in [theirs_a, theirs_b] {
        assert_eq!(runner.state().objects[&id].zone, Zone::Exile);
    }
    assert_eq!(
        runner.state().objects[&own_creature].zone,
        Zone::Battlefield
    );

    // "its controller searches": one search per exiled creature, by its controller.
    assert_eq!(
        searchers,
        vec![P1, P1],
        "only the exiled creatures' controller searches"
    );

    // The caster's library is untouched.
    assert_eq!(
        zone_names(&runner, &player(&runner, P0).library),
        vec!["Forest", "Shivan Dragon", "Sol Ring"],
    );
    assert_eq!(battlefield_names(&runner, P0), vec!["Grizzly Bears"]);

    // The opponent got exactly two basic lands, tapped; the rest stays in the library.
    let p1_field = battlefield_names(&runner, P1);
    assert_eq!(
        p1_field.len(),
        2,
        "two basics for two exiled creatures: {p1_field:?}"
    );
    for id in runner.state().battlefield.iter() {
        let obj = &runner.state().objects[id];
        if obj.controller == P1 {
            assert!(
                ["Plains", "Swamp", "Mountain"].contains(&obj.name.as_str()),
                "only basic lands enter: {}",
                obj.name
            );
            assert!(obj.tapped, "{} enters tapped", obj.name);
        }
    }
    let p1_library = zone_names(&runner, &player(&runner, P1).library);
    assert_eq!(p1_library.len(), 4, "{p1_library:?}");
    for kept in ["Shivan Dragon", "Sol Ring", "Command Tower"] {
        assert!(
            p1_library.iter().any(|n| n == kept),
            "{kept} stays in the library"
        );
    }
}
