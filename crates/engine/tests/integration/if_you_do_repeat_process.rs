//! Runtime regression for the bare "If you do, repeat this process" directive
//! (CR 608.2c + CR 118.12), via Insatiable Frugivore — "When this creature
//! enters, create a Food token, then you may exile three cards from your
//! graveyard. If you do, repeat this process." (Primal Surge has the same shape.)
//!
//! The directive used to be consumed with no loop predicate, so the trigger made
//! ONE Food. It now parses to a `WhileCondition` gated on the process's own
//! "you may" (`OptionalEffectPerformed`), evaluated per iteration. The
//! coordinated form "If you do, <instruction> and repeat this process"
//! (Cultivator Colossus) is the upstream's structural repeat directive
//! (`RepeatDirectiveReached`) since v0.118.0 and is covered by
//! `repeat_directive_gate`.

use engine::game::ability_utils::build_resolved_from_def;
use engine::game::effects::resolve_ability_chain;
use engine::game::scenario::{GameRunner, GameScenario, P0};
use engine::game::zones::create_object;
use engine::parser::parse_oracle_text;
use engine::types::ability::{RepeatContinuation, ResolvedAbility};
use engine::types::actions::GameAction;
use engine::types::card_type::{CoreType, Supertype};
use engine::types::game_state::{GameState, WaitingFor};
use engine::types::identifiers::{CardId, ObjectId};
use engine::types::phase::Phase;
use engine::types::zones::Zone;

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
