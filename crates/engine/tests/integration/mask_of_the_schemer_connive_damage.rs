//! Mask of the Schemer (relato de 07/11/2026): "Whenever equipped creature deals
//! combat damage to a player, it connives X, where X is the amount of damage it
//! dealt to that player."
//!
//! The where-X binder did not read "the amount of damage it dealt to that player",
//! so the trigger lowered to `Unimplemented { where_x_binding }` and resolved
//! doing nothing — the equipped creature dealt 5 and never connived.
//!
//! CR 701.50a + CR 701.50d: connive X draws X, discards X, and puts a +1/+1
//! counter on the conniver for each nonland card discarded. CR 608.2h: X is the
//! amount of the triggering damage event.

use engine::game::effects::attach::attach_to;
use engine::game::layers::evaluate_layers;
use engine::game::scenario::{GameScenario, P0, P1};
use engine::parser::oracle::parse_oracle_text;
use engine::types::ability::{Effect, QuantityExpr, QuantityRef};
use engine::types::actions::GameAction;
use engine::types::counter::CounterType;
use engine::types::game_state::WaitingFor;
use engine::types::phase::Phase;
use engine::types::triggers::TriggerMode;
use engine::types::zones::Zone;

use super::rules::run_combat;

const MASK_ORACLE: &str = "Whenever equipped creature deals combat damage to a player, it \
connives X, where X is the amount of damage it dealt to that player. (Draw X cards, then discard \
X cards. Put a +1/+1 counter on that creature for each nonland card discarded this way.)\nEquip {2}";

#[test]
fn mask_of_the_schemer_binds_x_to_the_damage_dealt() {
    let parsed = parse_oracle_text(
        MASK_ORACLE,
        "Mask of the Schemer",
        &[],
        &["Artifact".to_string()],
        &["Equipment".to_string()],
    );
    let trigger = parsed
        .triggers
        .iter()
        .find(|t| t.mode == TriggerMode::DamageDone)
        .expect("Mask must parse its combat-damage trigger");
    let execute = trigger.execute.as_ref().expect("trigger body");
    match execute.effect.as_ref() {
        Effect::Connive { count, .. } => assert_eq!(
            *count,
            QuantityExpr::Ref {
                qty: QuantityRef::EventContextAmount
            },
            "X is the triggering damage amount"
        ),
        other => panic!("expected Connive, got {other:?}"),
    }
}

#[test]
fn mask_of_the_schemer_connives_for_the_combat_damage() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);

    let attacker = scenario.add_creature(P0, "Schemer Bearer", 2, 2).id();
    let mask = scenario
        .add_creature(P0, "Mask of the Schemer", 0, 0)
        .as_artifact()
        .with_subtypes(vec!["Equipment"])
        .from_oracle_text(MASK_ORACLE)
        .id();
    // The two draws are nonland spells; a land already in hand gives the
    // discard a real choice.
    scenario.add_spell_to_library_top(P0, "Drawn Spell A", true);
    scenario.add_spell_to_library_top(P0, "Drawn Spell B", true);
    let kept_land = scenario.add_land_to_hand(P0, "Forest").id();

    let mut runner = scenario.build();
    attach_to(runner.state_mut(), mask, attacker);
    evaluate_layers(runner.state_mut());

    let life_before = runner.life(P1);
    run_combat(&mut runner, vec![attacker], vec![]);
    assert_eq!(runner.life(P1), life_before - 2, "2 combat damage");

    let mut discarded_prompt = false;
    for _ in 0..40 {
        match runner.state().waiting_for.clone() {
            WaitingFor::ConniveDiscard { cards, count, .. } => {
                assert_eq!(count, 2, "connive X = 2 discards two");
                let pick: Vec<_> = cards.into_iter().filter(|c| *c != kept_land).collect();
                assert_eq!(pick.len(), 2, "the two drawn spells are offered");
                runner
                    .act(GameAction::SelectCards { cards: pick })
                    .expect("discard the two drawn spells");
                discarded_prompt = true;
            }
            WaitingFor::Priority { .. } if !runner.state().stack.is_empty() => {
                runner.act(GameAction::PassPriority).expect("pass");
            }
            _ => break,
        }
    }
    assert!(discarded_prompt, "connive X must ask for the two discards");

    let hand: Vec<_> = runner
        .state()
        .players[0]
        .hand
        .iter()
        .copied()
        .collect();
    assert_eq!(hand, vec![kept_land], "drew 2, discarded the 2 spells");
    let counters = runner
        .state()
        .objects
        .get(&attacker)
        .map(|o| o.counters.get(&CounterType::Plus1Plus1).copied().unwrap_or(0))
        .unwrap_or(0);
    assert_eq!(counters, 2, "one +1/+1 counter per nonland card discarded");
    let in_graveyard = runner
        .state()
        .objects
        .values()
        .filter(|o| o.zone == Zone::Graveyard && o.owner == P0)
        .count();
    assert_eq!(in_graveyard, 2);
}
