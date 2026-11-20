//! Relatos da mesa de 20/11/2026 (rodada das 14h UTC).
//!
//! - Feed the Cycle: "As an additional cost to cast this spell, forage or pay
//!   {B}. (To forage, exile three cards from your graveyard or sacrifice a
//!   Food.) Destroy target creature or planeswalker." A mesa listava a
//!   conjuração e o motor a recusava.

use engine::ai_support::legal_actions;
use engine::game::rehydrate_game_from_card_db;
use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::game::scenario_db::GameScenarioDbExt;
use engine::types::actions::GameAction;
use engine::types::game_state::{CastPaymentMode, WaitingFor};
use engine::types::identifiers::ObjectId;
use engine::types::mana::{ManaType, ManaUnit};
use engine::types::phase::Phase;
use engine::types::player::PlayerId;
use engine::types::zones::Zone;

use crate::support::shared_card_db as load_db;

const FOOD: &str = "{2}, {T}, Sacrifice this artifact: You gain 3 life.";

fn add_pool(runner: &mut GameRunner, player: PlayerId, mana: &[ManaType]) {
    let pool = &mut runner
        .state_mut()
        .players
        .iter_mut()
        .find(|p| p.id == player)
        .unwrap()
        .mana_pool;
    for m in mana {
        pool.add(ManaUnit::new(*m, ObjectId(0), false, vec![]));
    }
}

/// Conjura e responde cada espera: as perguntas de custo com `decisions`, o
/// resto com a jogada legal que cita o elfo do oponente (o alvo). Devolve true
/// se a criatura foi destruída.
fn cast_feed(
    runner: &mut GameRunner,
    spell: ObjectId,
    victim: ObjectId,
    decisions: &[bool],
) -> bool {
    let mut decisions = decisions.iter().copied();
    let card_id = runner.state().objects[&spell].card_id;
    assert!(
        legal_actions(runner.state())
            .iter()
            .any(|a| matches!(a, GameAction::CastSpell { object_id, .. } if *object_id == spell)),
        "Feed the Cycle tem de ser listada"
    );
    runner
        .act(GameAction::CastSpell {
            object_id: spell,
            card_id,
            targets: vec![],
            payment_mode: CastPaymentMode::Auto,
        })
        .expect("Feed the Cycle listada tem de ser aceita");
    for _ in 0..30 {
        let w = runner.state().waiting_for.clone();
        if matches!(w, WaitingFor::Priority { .. }) {
            if runner.state().stack.is_empty() {
                break;
            }
            runner.act(GameAction::PassPriority).expect("pass");
            continue;
        }
        if matches!(w, WaitingFor::OptionalCostChoice { .. }) {
            let pay = decisions.next().expect("pergunta de custo a mais");
            runner
                .act(GameAction::DecideOptionalCost { pay })
                .expect("decisão de custo aceita");
            continue;
        }
        let legal = legal_actions(runner.state());
        let pick = legal
            .iter()
            .find(|a| format!("{a:?}").contains(&format!("ObjectId({})", victim.0)))
            .or_else(|| legal.first())
            .cloned()
            .expect("sem jogada legal");
        runner.act(pick).expect("jogada listada aceita");
    }
    assert!(decisions.next().is_none(), "faltou uma pergunta de custo");
    runner.state().objects.get(&victim).map(|o| o.zone) == Some(Zone::Graveyard)
}

fn food_count(runner: &GameRunner) -> usize {
    runner
        .state()
        .objects
        .values()
        .filter(|o| o.name == "Food" && o.zone == Zone::Battlefield)
        .count()
}

fn setup(db: &engine::database::card_db::CardDatabase) -> (GameRunner, ObjectId, ObjectId) {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let spell = scenario.add_real_card(P0, "Feed the Cycle", Zone::Hand, db);
    let victim = scenario.add_real_card(P1, "Llanowar Elves", Zone::Battlefield, db);
    let runner = scenario.build();
    (runner, spell, victim)
}

#[test]
fn feed_the_cycle_pay_black() {
    let Some(db) = load_db() else { return };
    let (mut runner, spell, victim) = setup(db);
    rehydrate_game_from_card_db(runner.state_mut(), db);
    add_pool(
        &mut runner,
        P0,
        &[ManaType::Black, ManaType::Black, ManaType::Black],
    );
    // Sem Food nem cemitério, só pagar {B}: sem pergunta.
    assert!(
        cast_feed(&mut runner, spell, victim, &[]),
        "pagando {{B}} o elfo morre"
    );
}

#[test]
fn feed_the_cycle_forage_food() {
    let Some(db) = load_db() else { return };
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let spell = scenario.add_real_card(P0, "Feed the Cycle", Zone::Hand, db);
    let victim = scenario.add_real_card(P1, "Llanowar Elves", Zone::Battlefield, db);
    scenario
        .add_artifact_from_oracle(P0, "Food", FOOD)
        .with_subtypes(vec!["Food"]);
    let mut runner = scenario.build();
    rehydrate_game_from_card_db(runner.state_mut(), db);
    assert_eq!(food_count(&runner), 1, "uma Food no campo");
    // Só {1}{B}: pagar {B} a mais não dá, forragear com a Food dá.
    add_pool(&mut runner, P0, &[ManaType::Black, ManaType::Green]);
    assert!(
        legal_actions(runner.state())
            .iter()
            .any(|a| matches!(a, GameAction::CastSpell { object_id, .. } if *object_id == spell)),
        "Feed the Cycle tem de ser conjurável forrageando"
    );
    assert!(
        cast_feed(&mut runner, spell, victim, &[true]),
        "com Food o elfo morre"
    );
    assert_eq!(food_count(&runner), 0, "a Food foi sacrificada");
}

#[test]
fn feed_the_cycle_forage_asks_which_way() {
    let Some(db) = load_db() else { return };
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let spell = scenario.add_real_card(P0, "Feed the Cycle", Zone::Hand, db);
    let victim = scenario.add_real_card(P1, "Llanowar Elves", Zone::Battlefield, db);
    scenario
        .add_artifact_from_oracle(P0, "Food", FOOD)
        .with_subtypes(vec!["Food"]);
    for _ in 0..3 {
        scenario.add_real_card(P0, "Grizzly Bears", Zone::Graveyard, db);
    }
    let mut runner = scenario.build();
    rehydrate_game_from_card_db(runner.state_mut(), db);
    add_pool(
        &mut runner,
        P0,
        &[ManaType::Black, ManaType::Black, ManaType::Black],
    );
    // 1ª pergunta: forragear (sim) ou pagar {B}; 2ª: exilar três do cemitério
    // (não) ou sacrificar a Food.
    assert!(
        cast_feed(&mut runner, spell, victim, &[true, false]),
        "o elfo morre"
    );
    assert_eq!(food_count(&runner), 0, "escolheu sacrificar a Food");
    let gy = runner
        .state()
        .players
        .iter()
        .find(|p| p.id == P0)
        .unwrap()
        .graveyard
        .len();
    assert!(gy >= 3, "o cemitério ficou (não exilou)");
}
