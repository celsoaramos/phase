//! Relatos da mesa de 20/11/2026 (rodada das 19h UTC).
//!
//! - Varchild, Betrayer of Kjeldor: "Survivors your opponents control can't
//!   block, and they can't attack you or planeswalkers you control." A linha
//!   virava um único `CantBlock { affected: SelfRef }`: o próprio Varchild não
//!   bloqueava e os Survivors do oponente atacavam o dono do Varchild.
//!   CR 508.1c + CR 509.1b: as duas restrições valem para o mesmo sujeito.

use super::rules::{AttackTarget, GameRunner, GameScenario, ObjectId, Phase, WaitingFor, P0, P1};

/// Texto oficial (Scryfall, C17).
const VARCHILD: &str = "Whenever Varchild deals combat damage to a player, that player creates that many 1/1 red Survivor creature tokens.\nSurvivors your opponents control can't block, and they can't attack you or planeswalkers you control.\nWhen Varchild leaves the battlefield, gain control of all Survivors.";

fn scenario_at_combat() -> GameScenario {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.with_library_top(P0, &["Filler A"]);
    scenario.with_library_top(P1, &["Filler B"]);
    scenario
}

fn add_varchild(
    scenario: &mut GameScenario,
    controller: engine::types::player::PlayerId,
) -> ObjectId {
    scenario
        .add_creature(controller, "Varchild, Betrayer of Kjeldor", 3, 3)
        .with_subtypes(vec!["Human", "Knight"])
        .from_oracle_text(VARCHILD)
        .id()
}

fn add_survivor(
    scenario: &mut GameScenario,
    controller: engine::types::player::PlayerId,
) -> ObjectId {
    scenario
        .add_creature(controller, "Survivor", 1, 1)
        .with_subtypes(vec!["Survivor"])
        .id()
}

fn declare(runner: &mut GameRunner, attacker: ObjectId) -> bool {
    runner.pass_both_players();
    runner
        .declare_attackers(&[(attacker, AttackTarget::Player(P1))])
        .is_ok()
}

fn to_blockers(runner: &mut GameRunner) {
    for _ in 0..12 {
        if matches!(
            runner.state().waiting_for,
            WaitingFor::DeclareBlockers { .. }
        ) {
            return;
        }
        runner.pass_both_players();
    }
    panic!(
        "never reached declare blockers: {:?}",
        runner.state().waiting_for
    );
}

#[test]
fn varchild_static_parses_into_two_scoped_restrictions() {
    let mut scenario = scenario_at_combat();
    let varchild = add_varchild(&mut scenario, P1);
    let runner = scenario.build();
    let statics: Vec<_> = runner.state().objects[&varchild]
        .static_definitions
        .iter_all()
        .collect();
    assert_eq!(statics.len(), 2, "{statics:#?}");
    assert!(
        statics.iter().all(|d| !matches!(
            d.affected,
            Some(engine::types::ability::TargetFilter::SelfRef)
        )),
        "nenhuma restrição pode cair no próprio Varchild: {statics:#?}"
    );
}

#[test]
fn opponents_survivor_cannot_attack_varchilds_controller() {
    let mut scenario = scenario_at_combat();
    add_varchild(&mut scenario, P1);
    let survivor = add_survivor(&mut scenario, P0);
    let mut runner = scenario.build();
    assert!(
        !declare(&mut runner, survivor),
        "Survivor do oponente não pode atacar o controlador do Varchild"
    );
}

#[test]
fn other_creatures_still_attack_and_varchild_still_blocks() {
    let mut scenario = scenario_at_combat();
    let varchild = add_varchild(&mut scenario, P1);
    let bear = scenario.add_creature(P0, "Grizzly Bears", 2, 2).id();
    let mut runner = scenario.build();
    assert!(
        declare(&mut runner, bear),
        "criatura que não é Survivor ataca"
    );
    to_blockers(&mut runner);
    runner
        .declare_blockers(&[(varchild, bear)])
        .expect("o próprio Varchild pode bloquear");
}

/// Passa a vez até o fim do combate e diz se o defensor recebeu a pergunta de
/// bloqueio (CR 509.1). Sem bloqueador possível o motor pula a pergunta.
fn blocker_prompt_offered(runner: &mut GameRunner) -> bool {
    for _ in 0..12 {
        if matches!(
            runner.state().waiting_for,
            WaitingFor::DeclareBlockers { .. }
        ) {
            return true;
        }
        if matches!(runner.state().phase, Phase::CombatDamage | Phase::EndCombat) {
            return false;
        }
        runner.pass_both_players();
    }
    false
}

#[test]
fn opponents_survivor_cannot_block() {
    // Controle: sem Varchild, o Survivor do defensor bloqueia normalmente.
    let mut scenario = scenario_at_combat();
    let bear = scenario.add_creature(P0, "Grizzly Bears", 2, 2).id();
    add_survivor(&mut scenario, P1);
    let mut runner = scenario.build();
    assert!(declare(&mut runner, bear));
    assert!(
        blocker_prompt_offered(&mut runner),
        "controle: o Survivor bloqueia"
    );

    let mut scenario = scenario_at_combat();
    add_varchild(&mut scenario, P0);
    let bear = scenario.add_creature(P0, "Grizzly Bears", 2, 2).id();
    add_survivor(&mut scenario, P1);
    let mut runner = scenario.build();
    assert!(declare(&mut runner, bear));
    assert!(
        !blocker_prompt_offered(&mut runner),
        "Survivor do oponente do Varchild não pode bloquear"
    );
}
