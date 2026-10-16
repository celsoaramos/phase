//! Gavony Dawnguard — "Whenever day becomes night or night becomes day, look at
//! the top four cards of your library…" never triggered: the trigger index keyed
//! `TriggerMode::DayTimeChanges` on `GameEvent::DayNightChanged`, but the matcher
//! only accepted an `EffectResolved { DayTimeChange }` that nothing emits.
//!
//! CR 731.1a: "day becomes night" / "night becomes day" is the game losing one
//! designation and gaining the other. CR 731.2: the check happens as the second
//! part of the untap step (the engine runs it as the turn ends).

use engine::game::day_night::resolve_set_day_night;
use engine::game::scenario::{GameRunner, GameScenario, P0};
use engine::types::actions::GameAction;
use engine::types::card_type::CoreType;
use engine::types::game_state::{DayNight, WaitingFor};
use engine::types::identifiers::ObjectId;
use engine::types::mana::ManaCost;
use engine::types::phase::Phase;

const DAWNGUARD_ORACLE: &str = "Ward {1}\n\
If it's neither day nor night, it becomes day as Gavony Dawnguard enters.\n\
Whenever day becomes night or night becomes day, look at the top four cards of your library. \
You may reveal a creature card with mana value 3 or less from among them and put it into your hand. \
Put the rest on the bottom of your library in any order.";

/// Dawnguard on P0's battlefield, four cards on top of P0's library (the third
/// is a two-mana creature, so the dig has something to offer).
fn dawnguard_scenario(phase: Phase) -> (GameRunner, ObjectId) {
    let mut scenario = GameScenario::new();
    scenario.at_phase(phase);
    scenario
        .add_creature(P0, "Gavony Dawnguard", 3, 3)
        .from_oracle_text(DAWNGUARD_ORACLE);
    scenario.with_library_top(P0, &["Top A", "Top B", "Small Bear", "Top D"]);
    let mut runner = scenario.build();
    let bear = runner
        .state()
        .objects
        .values()
        .find(|o| o.name == "Small Bear")
        .map(|o| o.id)
        .expect("library creature staged");
    let obj = runner.state_mut().objects.get_mut(&bear).unwrap();
    obj.card_types.core_types.push(CoreType::Creature);
    obj.base_card_types = obj.card_types.clone();
    obj.mana_cost = ManaCost::generic(2);
    obj.power = Some(2);
    obj.toughness = Some(2);
    (runner, bear)
}

/// Pass priority until the Dawnguard's dig asks P0, or give up.
fn pass_until_dig(runner: &mut GameRunner) -> Option<Vec<ObjectId>> {
    for _ in 0..64 {
        match &runner.state().waiting_for {
            WaitingFor::DigChoice { player, cards, .. } if *player == P0 => {
                return Some(cards.clone());
            }
            WaitingFor::GameOver { .. } => return None,
            _ => {
                if runner.act(GameAction::PassPriority).is_err() {
                    return None;
                }
            }
        }
    }
    None
}

#[test]
fn day_becoming_night_at_turn_end_triggers_the_dig() {
    let (mut runner, bear) = dawnguard_scenario(Phase::End);
    // CR 731.2a: it's day and the active player cast no spells this turn.
    runner.state_mut().day_night = Some(DayNight::Day);

    let cards = pass_until_dig(&mut runner)
        .expect("day → night must trigger Gavony Dawnguard's look-at-the-top-four");
    assert_eq!(runner.state().day_night, Some(DayNight::Night));
    assert_eq!(cards.len(), 4, "looks at the top four cards");
    assert!(cards.contains(&bear));

    runner
        .act(GameAction::SelectCards { cards: vec![bear] })
        .expect("keep the two-mana creature");
    // The rest go to the bottom "in any order" — answer any ordering prompt.
    for _ in 0..8 {
        if matches!(runner.state().waiting_for, WaitingFor::Priority { .. }) {
            break;
        }
        let _ = runner.act(GameAction::PassPriority);
    }
    let hand = &runner.state().players[P0.0 as usize].hand;
    assert!(
        hand.contains(&bear),
        "the revealed creature goes to the hand"
    );
}

#[test]
fn effect_making_it_night_triggers_the_dig() {
    let (mut runner, _) = dawnguard_scenario(Phase::PreCombatMain);
    runner.state_mut().day_night = Some(DayNight::Day);

    let mut events = Vec::new();
    resolve_set_day_night(runner.state_mut(), DayNight::Night, &mut events);
    engine::game::triggers::process_triggers(runner.state_mut(), &events);
    assert!(
        !runner.state().stack.is_empty() || runner.state().pending_trigger.is_some(),
        "an effect turning day into night is also \"day becomes night\""
    );
}

#[test]
fn first_designation_and_reasserting_it_do_not_trigger() {
    let (mut runner, _) = dawnguard_scenario(Phase::PreCombatMain);

    // CR 731.1: neither → day is the game's FIRST designation, not "night becomes day".
    let mut events = Vec::new();
    engine::game::day_night::initialize_day_night(runner.state_mut(), &mut events);
    // An effect saying "it becomes day" while it's already day changes nothing.
    resolve_set_day_night(runner.state_mut(), DayNight::Day, &mut events);
    engine::game::triggers::process_triggers(runner.state_mut(), &events);
    assert!(runner.state().stack.is_empty());
    assert!(runner.state().pending_trigger.is_none());
}
