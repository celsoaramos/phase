//! Relatos da mesa de 17/11/2026, 2ª leva (motor v0.125.0).
//!
//! - **The Ooze**: "Whenever a creature you control with a +1/+1 counter on it
//!   leaves the battlefield, create a Mutagen token for each +1/+1 counter on
//!   it." — o gatilho resolvia e não criava ficha nenhuma: "counter on it" era
//!   lido como os marcadores da própria Ooze, não da criatura que saiu.
//!   Cleopatra, Exiled Pharaoh ("draw a card for each counter on it") tem a
//!   mesma forma.
//! - **Transwarp Hub**: "mill five cards. You may put up to two artifact cards
//!   from among them into your hand." — só deixava escolher UMA carta (o "up to
//!   two" não chegava à seleção). Smuggler's Surprise tem a mesma forma.

use engine::game::scenario::{GameRunner, GameScenario, P0};
use engine::parser::oracle::parse_oracle_text;
use engine::types::ability::{Effect, ObjectScope, QuantityExpr, QuantityRef};
use engine::types::actions::GameAction;
use engine::types::game_state::{CastPaymentMode, WaitingFor};
use engine::types::identifiers::ObjectId;
use engine::types::mana::ManaCost;
use engine::types::phase::Phase;
use engine::types::zones::Zone;

const THE_OOZE: &str = "Whenever a creature you control with a +1/+1 counter on it leaves the \
battlefield, create a Mutagen token for each +1/+1 counter on it. (A Mutagen token is an artifact \
with \"{1}, {T}, Sacrifice this token: Put a +1/+1 counter on target creature. Activate only as a \
sorcery.\")\n{T}: Exile target card from a graveyard. Create a Mutagen token.";

const CLEOPATRA: &str = "Allies — At the beginning of your end step, put a +1/+1 counter on each \
of up to two other target legendary creatures.\nBetrayal — Whenever a legendary creature with \
counters on it dies, draw a card for each counter on it. You lose 2 life.";

const TRANSWARP_HUB: &str = "When this Spacecraft enters, mill five cards. You may put up to two \
artifact cards from among them into your hand.\nStation (Tap another creature you control: Put \
charge counters equal to its power on this Spacecraft. Station only as a sorcery.)";

const FREE: ManaCost = ManaCost::Cost {
    shards: vec![],
    generic: 0,
};

fn zone_of(runner: &GameRunner, id: ObjectId) -> Zone {
    runner.state().objects[&id].zone
}

fn cast(runner: &mut GameRunner, id: ObjectId, targets: Vec<ObjectId>) {
    let card_id = runner.state().objects[&id].card_id;
    runner
        .act(GameAction::CastSpell {
            object_id: id,
            card_id,
            targets,
            payment_mode: CastPaymentMode::Auto,
        })
        .expect("cast");
}

fn token_count(runner: &GameRunner, name: &str) -> usize {
    runner
        .state()
        .objects
        .values()
        .filter(|o| o.zone == Zone::Battlefield && o.name == name)
        .count()
}

/// Every `CountersOn` scope reachable from the first trigger's execute chain.
fn counter_scopes(oracle: &str, name: &str, types: &[&str], trigger_index: usize) -> Vec<ObjectScope> {
    let types: Vec<String> = types.iter().map(|t| t.to_string()).collect();
    let parsed = parse_oracle_text(oracle, name, &[], &types, &[]);
    let trigger = &parsed.triggers[trigger_index];
    let mut scopes = Vec::new();
    let mut ability = trigger.execute.as_deref();
    while let Some(def) = ability {
        let mut visit = |expr: &QuantityExpr| {
            let mut expr = expr.clone();
            expr.each_ref_mut(&mut |qty: &mut QuantityRef| {
                if let QuantityRef::CountersOn { scope, .. } = qty {
                    scopes.push(*scope);
                }
            });
        };
        match def.effect.as_ref() {
            Effect::Token { count, .. } | Effect::Draw { count, .. } => visit(count),
            _ => {}
        }
        ability = def.sub_ability.as_deref();
    }
    scopes
}

#[test]
fn the_ooze_counts_the_counters_on_the_departing_creature() {
    assert_eq!(
        counter_scopes(THE_OOZE, "The Ooze", &["Legendary", "Artifact"], 0),
        vec![ObjectScope::EventSource],
        "\"for each +1/+1 counter on it\" — \"it\" is the creature that left"
    );
}

#[test]
fn cleopatra_draws_for_the_counters_on_the_dead_legend() {
    assert_eq!(
        counter_scopes(
            CLEOPATRA,
            "Cleopatra, Exiled Pharaoh",
            &["Legendary", "Creature"],
            1
        ),
        vec![ObjectScope::EventSource],
        "\"draw a card for each counter on it\" — \"it\" is the legendary creature that died"
    );
}

#[test]
fn the_ooze_creates_a_mutagen_for_each_counter_on_the_creature_that_left() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.add_artifact_from_oracle(P0, "The Ooze", THE_OOZE);
    let bear = {
        let mut card = scenario.add_creature(P0, "Grizzly Bears", 2, 2);
        card.with_plus_counters(3);
        card.id()
    };
    let murder = {
        let mut card =
            scenario.add_spell_to_hand_from_oracle(P0, "Murder", true, "Destroy target creature.");
        card.with_mana_cost(FREE);
        card.id()
    };
    let mut runner = scenario.build();
    cast(&mut runner, murder, vec![bear]);
    for _ in 0..40 {
        match runner.state().waiting_for.clone() {
            WaitingFor::Priority { .. } if runner.state().stack.is_empty() => break,
            WaitingFor::Priority { .. } | WaitingFor::ManaPayment { .. } => {
                runner.act(GameAction::PassPriority).expect("pass");
            }
            other => panic!("unexpected prompt: {other:?}"),
        }
    }
    assert_eq!(zone_of(&runner, bear), Zone::Graveyard);
    assert_eq!(
        token_count(&runner, "Mutagen"),
        3,
        "three +1/+1 counters on the creature that left → three Mutagen tokens"
    );
}

#[test]
fn transwarp_hub_lets_you_take_two_milled_artifacts() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let mut artifacts = Vec::new();
    for name in ["Opt", "Sol Ring", "Shock", "Mind Stone", "Arcane Signet"] {
        let mut card = scenario.add_spell_to_library_top(P0, name, true);
        if name != "Opt" && name != "Shock" {
            card.as_artifact();
            artifacts.push(card.id());
        }
    }
    let hub = {
        let mut card = scenario.add_artifact_to_hand_from_oracle(P0, "Transwarp Hub", TRANSWARP_HUB);
        card.with_mana_cost(FREE);
        card.id()
    };
    let mut runner = scenario.build();
    cast(&mut runner, hub, vec![]);
    let mut chosen = false;
    for _ in 0..60 {
        match runner.state().waiting_for.clone() {
            WaitingFor::Priority { .. } if runner.state().stack.is_empty() => break,
            WaitingFor::Priority { .. } | WaitingFor::ManaPayment { .. } => {
                runner.act(GameAction::PassPriority).expect("pass");
            }
            WaitingFor::TargetSelection { .. } | WaitingFor::TriggerTargetSelection { .. } => {
                panic!("selection prompt shape: {:?}", runner.state().waiting_for);
            }
            other => {
                // The resolution-time choice of milled cards: pick two artifacts.
                assert!(!chosen, "asked twice: {other:?}");
                chosen = true;
                runner
                    .act(GameAction::SelectCards {
                        cards: artifacts[..2].to_vec(),
                    })
                    .unwrap_or_else(|e| panic!("two artifacts rejected ({e:?}) at {other:?}"));
            }
        }
    }
    assert!(chosen, "Transwarp Hub never offered the milled artifacts");
    assert_eq!(zone_of(&runner, artifacts[0]), Zone::Hand);
    assert_eq!(zone_of(&runner, artifacts[1]), Zone::Hand);
    assert_eq!(zone_of(&runner, artifacts[2]), Zone::Graveyard);
}

const VIRTUE_OF_STRENGTH: &str =
    "If you tap a basic land for mana, it produces three times as much of that mana instead.";

/// Relato "Virtude da Força não funciona": a Forest virada à mão com a Virtude no
/// campo gera {G}{G}{G}. Trava de regressão — o motor já fazia isso.
#[test]
fn virtue_of_strength_triples_a_basic_tapped_by_hand() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.add_enchantment_from_oracle(P0, "Virtue of Strength", VIRTUE_OF_STRENGTH);
    let forest = scenario.add_basic_land(P0, engine::types::mana::ManaColor::Green);
    let mut runner = scenario.build();
    let (_, _, grouped) = engine::ai_support::legal_actions_full(runner.state());
    let selection = grouped
        .get(&forest)
        .into_iter()
        .flatten()
        .find_map(|action| match action {
            GameAction::TapLandForMana { selection } => Some(selection.clone()),
            _ => None,
        })
        .expect("the Forest can be tapped for mana");
    runner
        .act(GameAction::TapLandForMana { selection })
        .expect("tap the Forest");
    assert!(
        matches!(runner.state().waiting_for, WaitingFor::Priority { .. }),
        "no prompt after tapping: {:?}",
        runner.state().waiting_for
    );
    assert_eq!(runner.state().players[0].mana_pool.total(), 3);
}
