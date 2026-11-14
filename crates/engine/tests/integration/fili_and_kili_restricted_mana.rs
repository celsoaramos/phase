//! Fíli and Kíli, Joyous: "{T}: Add {R}{R}. Spend this mana only to cast Dwarf,
//! Equipment, and Saga spells."
//!
//! CR 106.6: the restriction names the SET of objects the mana may be spent on.
//! Before the serial comma was read as a connective, the phrase collapsed into a
//! single alternative — an object that is at once a Dwarf, an Equipment and a
//! Saga — and the {R}{R} entered the pool and paid for nothing. Reported from
//! play: four red floating and a Dwarf still uncastable.
//!
//! These drive the real card through `legal_actions_full` and an `Auto` cast, the
//! path the client and the AI take, on top of the unit tests in `types/mana.rs`.

use engine::ai_support::legal_actions_full;
use engine::game::scenario::{GameRunner, GameScenario, P0};
use engine::game::scenario_db::GameScenarioDbExt;
use engine::types::actions::GameAction;
use engine::types::game_state::{CastPaymentMode, GameState, WaitingFor};
use engine::types::identifiers::ObjectId;
use engine::types::phase::Phase;
use engine::types::zones::Zone;

use crate::support::shared_card_db;

/// Fíli and Kíli plus `mountains` untapped Mountains, `hand` in hand.
fn board(mountains: usize, hand: &[&str]) -> Option<(GameRunner, ObjectId, Vec<ObjectId>)> {
    let db = shared_card_db()?;
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let fili = scenario.add_real_card(P0, "Fíli and Kíli, Joyous", Zone::Battlefield, db);
    for _ in 0..mountains {
        scenario.add_real_card(P0, "Mountain", Zone::Battlefield, db);
    }
    let hand = hand
        .iter()
        .map(|name| scenario.add_real_card(P0, name, Zone::Hand, db))
        .collect();
    let mut runner = scenario.build();
    engine::game::rehydrate_game_from_card_db(runner.state_mut(), db);
    let turn = runner.state().turn_number;
    for (_, obj) in runner.state_mut().objects.iter_mut() {
        if obj.zone == Zone::Battlefield {
            obj.tapped = false;
            obj.summoning_sick = false;
            obj.entered_battlefield_turn = Some(turn.saturating_sub(1));
        }
    }
    engine::game::layers::flush_layers(runner.state_mut());
    Some((runner, fili, hand))
}

fn listed_cast(state: &GameState, spell: ObjectId) -> Option<GameAction> {
    let (flat, _, _) = legal_actions_full(state);
    flat.into_iter().find(|a| {
        matches!(a, GameAction::CastSpell { .. }) && a.related_object_ids().contains(&spell)
    })
}

/// Cast through the listed action with automatic payment, then resolve the stack.
fn cast_and_resolve(runner: &mut GameRunner, spell: ObjectId) {
    let card_id = runner.state().objects[&spell].card_id;
    runner
        .act(GameAction::CastSpell {
            object_id: spell,
            card_id,
            targets: vec![],
            payment_mode: CastPaymentMode::Auto,
        })
        .expect("listed cast must be accepted");
    for _ in 0..16 {
        match &runner.state().waiting_for {
            WaitingFor::Priority { .. } if runner.state().stack.is_empty() => return,
            _ => {
                let _ = runner.act(GameAction::PassPriority);
            }
        }
    }
}

/// The reported board: the Dwarf is castable with Fíli's {R}{R} plus two Mountains,
/// a vanilla creature of the same cost is not (only two of the four mana are free).
#[test]
fn fili_and_kili_mana_casts_a_dwarf_and_nothing_unlisted() {
    let Some((mut runner, fili, hand)) = board(2, &["Dwarven Driller", "Hill Giant"]) else {
        return;
    };
    let (driller, giant) = (hand[0], hand[1]);

    assert!(
        listed_cast(runner.state(), driller).is_some(),
        "Dwarven Driller ({{3}}{{R}}, a Dwarf) must be castable with Fíli's {{R}}{{R}} + 2 Mountains"
    );
    assert!(
        listed_cast(runner.state(), giant).is_none(),
        "Hill Giant is not a Dwarf, an Equipment or a Saga: Fíli's mana cannot pay for it"
    );

    cast_and_resolve(&mut runner, driller);
    let state = runner.state();
    assert_eq!(state.objects[&driller].zone, Zone::Battlefield);
    assert!(state.objects[&fili].tapped, "the Auto payment must use Fíli's mana");
    assert!(
        state
            .objects
            .values()
            .filter(|o| o.zone == Zone::Battlefield && o.name == "Mountain")
            .all(|o| o.tapped),
        "{{3}}{{R}} = Fíli's {{R}}{{R}} + both Mountains"
    );
}

/// The other two listed kinds: an Equipment and a Saga. A colorless artifact that
/// is not an Equipment stays out.
#[test]
fn fili_and_kili_mana_casts_equipment_and_saga() {
    let Some((runner, _, hand)) = board(0, &["Bonesplitter", "Sol Ring"]) else {
        return;
    };
    assert!(
        listed_cast(runner.state(), hand[0]).is_some(),
        "Bonesplitter (Equipment) must be castable with Fíli's mana alone"
    );
    assert!(
        listed_cast(runner.state(), hand[1]).is_none(),
        "Sol Ring is an artifact but not an Equipment"
    );

    let Some((mut runner, _, hand)) = board(1, &["Fable of the Mirror-Breaker"]) else {
        return;
    };
    let fable = hand[0];
    assert!(
        listed_cast(runner.state(), fable).is_some(),
        "Fable of the Mirror-Breaker ({{2}}{{R}}, a Saga) = Fíli's {{R}}{{R}} + a Mountain"
    );
    cast_and_resolve(&mut runner, fable);
    assert_eq!(runner.state().objects[&fable].zone, Zone::Battlefield);
}
