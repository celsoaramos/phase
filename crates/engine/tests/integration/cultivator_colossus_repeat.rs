//! Runtime regression for "If you do, … repeat this process" (CR 608.2c +
//! CR 118.12), via Cultivator Colossus — "When this creature enters, you may put
//! a land card from your hand onto the battlefield tapped. If you do, draw a card
//! and repeat this process."
//!
//! The directive used to be consumed with no loop predicate, so the trigger put
//! ONE land and drew ONE card. It now parses to a `WhileCondition` gated on the
//! process's own "you may" (`OptionalEffectPerformed`), evaluated per iteration.

use engine::game::ability_utils::build_resolved_from_def;
use engine::game::effects::resolve_ability_chain;
use engine::game::scenario::{GameRunner, GameScenario, P0};
use engine::game::zones::create_object;
use engine::parser::parse_oracle_text;
use engine::types::ability::{
    AbilityCondition, EffectOutcomeSignal, RepeatContinuation, ResolvedAbility,
};
use engine::types::actions::GameAction;
use engine::types::card_type::{CoreType, Supertype};
use engine::types::game_state::{GameState, WaitingFor};
use engine::types::identifiers::{CardId, ObjectId};
use engine::types::phase::Phase;
use engine::types::zones::Zone;

const COLOSSUS_ORACLE: &str = "Trample\nCultivator Colossus's power and toughness are each equal to the number of lands you control.\nWhen this creature enters, you may put a land card from your hand onto the battlefield tapped. If you do, draw a card and repeat this process.";

fn colossus_trigger_ability(source: ObjectId) -> ResolvedAbility {
    let parsed = parse_oracle_text(
        COLOSSUS_ORACLE,
        "Cultivator Colossus",
        &["Trample".to_string()],
        &["Creature".to_string()],
        &["Plant".to_string(), "Beast".to_string()],
    );
    let def = parsed
        .triggers
        .iter()
        .find_map(|t| t.execute.clone())
        .expect("Cultivator Colossus's enters trigger has an execute body");
    assert_eq!(
        def.repeat_until,
        Some(RepeatContinuation::WhileCondition {
            condition: Box::new(AbilityCondition::EffectOutcome {
                signal: EffectOutcomeSignal::OptionalEffectPerformed,
            }),
            max_iterations: None,
        }),
        "precondition: the trigger repeats while its \"you may\" was performed"
    );
    build_resolved_from_def(&def, source, P0)
}

fn add_card(runner: &mut GameRunner, name: &str, zone: Zone, land: bool) -> ObjectId {
    let card_id = CardId(runner.state().next_object_id);
    let id = create_object(runner.state_mut(), card_id, P0, name.to_string(), zone);
    let obj = runner.state_mut().objects.get_mut(&id).unwrap();
    if land {
        obj.card_types.core_types.push(CoreType::Land);
        obj.card_types.supertypes.push(Supertype::Basic);
        obj.card_types.subtypes.push("Forest".to_string());
    } else {
        obj.card_types.core_types.push(CoreType::Instant);
    }
    obj.base_card_types = obj.card_types.clone();
    id
}

fn lands_on_battlefield(state: &GameState) -> usize {
    state
        .objects
        .values()
        .filter(|o| {
            o.zone == Zone::Battlefield
                && o.controller == P0
                && o.card_types.core_types.contains(&CoreType::Land)
        })
        .count()
}

fn hand_size(state: &GameState) -> usize {
    state.players[0].hand.len()
}

/// Answer each prompt: accept the "you may" while `accepts` lasts (then decline),
/// and pick the first offered cards at the card choice.
fn drive(runner: &mut GameRunner, accepts: usize) -> usize {
    drive_picking(runner, accepts, true)
}

fn drive_picking(runner: &mut GameRunner, mut accepts: usize, pick: bool) -> usize {
    let mut prompts = 0;
    for _ in 0..64 {
        let action = match &runner.state().waiting_for {
            WaitingFor::OptionalEffectChoice { .. } => {
                prompts += 1;
                let accept = accepts > 0;
                accepts = accepts.saturating_sub(1);
                GameAction::DecideOptionalEffect { accept }
            }
            WaitingFor::EffectZoneChoice { cards, count, .. } => GameAction::SelectCards {
                cards: if pick {
                    cards.iter().copied().take(*count).collect()
                } else {
                    Vec::new()
                },
            },
            WaitingFor::Priority { .. } => return prompts,
            other => panic!("unexpected prompt: {other:?}"),
        };
        runner.act(action).expect("prompt should resolve");
    }
    panic!("did not reach priority");
}

fn setup(hand_lands: usize, library: &[bool]) -> (GameRunner, ResolvedAbility) {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let mut runner = scenario.build();
    let source = add_card(&mut runner, "Cultivator Colossus", Zone::Battlefield, false);
    for _ in 0..hand_lands {
        add_card(&mut runner, "Forest", Zone::Hand, true);
    }
    for &land in library {
        add_card(&mut runner, if land { "Forest" } else { "Opt" }, Zone::Library, land);
    }
    let ability = colossus_trigger_ability(source);
    (runner, ability)
}

#[test]
fn colossus_repeats_until_no_land_left_in_hand() {
    // Three lands in hand, a library of spells: three lands enter, three cards drawn.
    let (mut runner, ability) = setup(3, &[false; 6]);
    let mut events = Vec::new();
    resolve_ability_chain(runner.state_mut(), &ability, &mut events, 0).unwrap();
    drive(&mut runner, usize::MAX);
    assert_eq!(lands_on_battlefield(runner.state()), 3);
    assert_eq!(hand_size(runner.state()), 3, "one draw per land put");
}

#[test]
fn colossus_keeps_going_with_a_drawn_land() {
    // One land in hand; the draws find two more lands, then a spell.
    let (mut runner, ability) = setup(1, &[true, true, false, false]);
    let mut events = Vec::new();
    resolve_ability_chain(runner.state_mut(), &ability, &mut events, 0).unwrap();
    drive(&mut runner, usize::MAX);
    assert_eq!(lands_on_battlefield(runner.state()), 3);
    assert_eq!(hand_size(runner.state()), 1, "the last draw is a spell");
}

#[test]
fn colossus_stops_when_declined() {
    let (mut runner, ability) = setup(3, &[false; 6]);
    let mut events = Vec::new();
    resolve_ability_chain(runner.state_mut(), &ability, &mut events, 0).unwrap();
    let prompts = drive(&mut runner, 1);
    assert_eq!(prompts, 2, "accepted once, declined the repeat");
    assert_eq!(lands_on_battlefield(runner.state()), 1);
    assert_eq!(hand_size(runner.state()), 3, "two lands left + one draw");
}

#[test]
fn colossus_without_land_in_hand_does_nothing() {
    let (mut runner, ability) = setup(0, &[false; 3]);
    let mut events = Vec::new();
    resolve_ability_chain(runner.state_mut(), &ability, &mut events, 0).unwrap();
    drive(&mut runner, usize::MAX);
    assert_eq!(lands_on_battlefield(runner.state()), 0);
    assert_eq!(hand_size(runner.state()), 0, "no land put, no draw");
}

const FRUGIVORE_ORACLE: &str = "When this creature enters, create a Food token, then you may exile three cards from your graveyard. If you do, repeat this process.";

fn frugivore_trigger_ability(source: ObjectId) -> ResolvedAbility {
    let parsed = parse_oracle_text(
        FRUGIVORE_ORACLE,
        "Insatiable Frugivore",
        &[],
        &["Creature".to_string()],
        &[],
    );
    let def = parsed
        .triggers
        .iter()
        .find_map(|t| t.execute.clone())
        .expect("Insatiable Frugivore's enters trigger has an execute body");
    assert!(
        matches!(def.repeat_until, Some(RepeatContinuation::WhileCondition { .. })),
        "precondition: the trigger repeats, got {:?}",
        def.repeat_until
    );
    build_resolved_from_def(&def, source, P0)
}

fn foods(state: &GameState) -> usize {
    state
        .objects
        .values()
        .filter(|o| o.zone == Zone::Battlefield && o.name == "Food")
        .count()
}

#[test]
fn frugivore_repeats_only_while_three_cards_can_be_exiled() {
    // Seven cards in the graveyard: Food + exile 3, Food + exile 3, Food — the
    // last "you may" cannot exile three, so the process ends with three Foods.
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let mut runner = scenario.build();
    let source = add_card(&mut runner, "Insatiable Frugivore", Zone::Battlefield, false);
    for _ in 0..7 {
        add_card(&mut runner, "Opt", Zone::Graveyard, false);
    }
    let ability = frugivore_trigger_ability(source);
    let mut events = Vec::new();
    resolve_ability_chain(runner.state_mut(), &ability, &mut events, 0).unwrap();
    drive(&mut runner, usize::MAX);
    assert_eq!(foods(runner.state()), 3);
    assert_eq!(runner.state().players[0].graveyard.len(), 1);
}
