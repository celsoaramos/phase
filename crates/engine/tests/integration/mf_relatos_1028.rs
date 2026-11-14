//! Relatos da mesa de 28/10/2026 (desafio do dia: Mono-Red Prowess).
//!
//! 1. CR 704.3 + CR 616.1: com DUAS Etching of Kumano, a criatura que morreria depois de
//!    dano de COMBATE pede ao controlador dela a ordem das duas substituições. O combate
//!    trocava essa pergunta pela prioridade: `pending_replacement` ficava pendurado, todo
//!    SBA seguinte era pausado (nenhuma criatura morria mais) e a partida travava na
//!    Limpeza com uma resolução sem dono (`resolving_stack_entry`).
//! 2. CR 702.145d: um permanente com daybound entrando sem dia nem noite faz virar dia.
//!    Ninguém iniciava a designação, e o Brutal Cathar sozinho nunca transformava.

use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::types::actions::GameAction;
use engine::types::game_state::{GameState, WaitingFor};
use engine::types::identifiers::ObjectId;
use engine::types::mana::{ManaType, ManaUnit};
use engine::types::phase::Phase;
use engine::types::zones::Zone;

use engine::game::scenario_db::GameScenarioDbExt;

use super::support::shared_card_db;

fn build(scenario: GameScenario) -> GameRunner {
    let db = shared_card_db().expect("card db");
    let mut runner = scenario.build();
    engine::game::rehydrate_game_from_card_db(runner.state_mut(), db);
    runner
}

fn pool(color: ManaType, n: usize) -> Vec<ManaUnit> {
    (0..n)
        .map(|_| ManaUnit::new(color, ObjectId(0), false, vec![]))
        .collect()
}

fn dump(tag: &str, s: &GameState) {
    eprintln!(
        "[{tag}] phase={:?} waiting={:?} stack={} resolving={:?} rstack={} pending_repl={:?}",
        s.phase,
        std::mem::discriminant(&s.waiting_for),
        s.stack.len(),
        s.resolving_stack_entry.as_ref().map(|e| e.id),
        s.resolution_stack.len(),
        s.pending_replacement.is_some(),
    );
}

fn assert_settled(tag: &str, s: &GameState) {
    dump(tag, s);
    assert!(
        s.resolving_stack_entry.is_none(),
        "{tag}: resolving_stack_entry preso com a pilha em {} e espera {:?}",
        s.stack.len(),
        s.waiting_for
    );
}

fn drive(r: &mut GameRunner, target: Option<ObjectId>, tag: &str) {
    for i in 0..30 {
        match &r.state().waiting_for {
            WaitingFor::TargetSelection { .. } | WaitingFor::TriggerTargetSelection { .. } => {
                let res = r.act(GameAction::ChooseTarget {
                    target: target.map(engine::types::ability::TargetRef::Object),
                });
                eprintln!(
                    "choose target -> {:?}",
                    res.as_ref().map(|r| &r.waiting_for)
                );
            }
            WaitingFor::Priority { .. } if !r.state().stack.is_empty() => {
                let res = r.act(GameAction::PassPriority);
                if let Ok(res) = res {
                    for e in res.events {
                        eprintln!(
                            "    ev {}",
                            format!("{e:?}").chars().take(160).collect::<String>()
                        );
                    }
                }
            }
            _ => break,
        }
        dump(&format!("{tag}{i}"), r.state());
    }
}

/// Hipótese: DUAS Etching of Kumano + criatura que morreria depois de dano de fonte do dono delas.
fn etchings_bolt(etchings: usize) -> GameRunner {
    let db = shared_card_db().expect("card db");
    let mut sc = GameScenario::new();
    sc.at_phase(Phase::PreCombatMain);
    for _ in 0..etchings {
        sc.add_real_card(P0, "Etching of Kumano", Zone::Battlefield, db);
    }
    let victim = sc.add_real_card(P1, "Grizzly Bears", Zone::Battlefield, db);
    let strike = sc.add_real_card(P0, "Lightning Strike", Zone::Hand, db);
    sc.with_mana_pool(P0, pool(ManaType::Red, 2));
    let mut r = build(sc);
    eprintln!(
        "campo: {:?}",
        r.state()
            .battlefield
            .iter()
            .map(|id| r.state().objects[id].name.clone())
            .collect::<Vec<_>>()
    );
    let _ = r.cast(strike).target_object(victim).commit();
    drive(&mut r, Some(victim), "et");
    eprintln!("waiting final: {:?}", r.state().waiting_for);
    eprintln!(
        "vítima em {:?}",
        r.state().objects.get(&victim).map(|o| o.zone)
    );
    r
}

#[test]
fn one_etching_exiles() {
    let r = etchings_bolt(1);
    assert_settled("1 etching", r.state());
}

#[test]
fn two_etchings_answered_settles() {
    let mut r = etchings_bolt(2);
    for i in 0..6 {
        if !matches!(r.state().waiting_for, WaitingFor::ReplacementChoice { .. }) {
            break;
        }
        let res = r.act(GameAction::ChooseReplacement { index: 0 });
        match res {
            Ok(res) => {
                for e in res.events {
                    eprintln!(
                        "    ev {}",
                        format!("{e:?}").chars().take(160).collect::<String>()
                    );
                }
            }
            Err(e) => eprintln!("ERR {e:?}"),
        }
        dump(&format!("ans{i}"), r.state());
    }
    eprintln!("waiting: {:?}", r.state().waiting_for);
    // passa a vez algumas vezes como a mesa faria
    for i in 0..4 {
        let res = r.act(GameAction::PassPriority);
        eprintln!(
            "pass{i}: {:?}",
            res.as_ref()
                .map(|x| x
                    .events
                    .iter()
                    .map(|e| format!("{e:?}").chars().take(80).collect::<String>())
                    .collect::<Vec<_>>())
                .map_err(|e| format!("{e:?}"))
        );
        dump(&format!("p{i}"), r.state());
    }
    assert_settled("2 etchings answered", r.state());
}

/// Relatos 2/7: dano de COMBATE do atacante do dono das Etchings mata o bloqueador.
fn etchings_combat(etchings: usize) -> (GameRunner, ObjectId, ObjectId) {
    etchings_combat_fs(etchings, false)
}

fn etchings_combat_fs(etchings: usize, first_strike: bool) -> (GameRunner, ObjectId, ObjectId) {
    let db = shared_card_db().expect("card db");
    let mut sc = GameScenario::new();
    sc.at_phase(Phase::PreCombatMain);
    for _ in 0..etchings {
        sc.add_real_card(P0, "Etching of Kumano", Zone::Battlefield, db);
    }
    let attacker = if first_strike {
        sc.add_creature(P0, "Big Attacker", 4, 4)
            .first_strike()
            .id()
    } else {
        sc.add_creature(P0, "Big Attacker", 4, 4).id()
    };
    let blocker = sc.add_creature(P1, "Small Blocker", 2, 2).id();
    let mut r = build(sc);
    r.advance_to_combat();
    dump("combat", r.state());
    (r, attacker, blocker)
}

fn run_combat(etchings: usize) -> GameRunner {
    run_combat_fs(etchings, false).0
}

fn run_combat_fs(etchings: usize, first_strike: bool) -> (GameRunner, ObjectId, ObjectId) {
    let (mut r, attacker, blocker) = etchings_combat_fs(etchings, first_strike);
    let res = r.act(GameAction::PassPriority);
    eprintln!("pass -> {:?}", res.map(|x| x.waiting_for));
    dump("decl?", r.state());
    let res = r.declare_attackers(&[(attacker, engine::game::combat::AttackTarget::Player(P1))]);
    eprintln!(
        "attack -> {:?}",
        res.as_ref()
            .map(|x| &x.waiting_for)
            .map_err(|e| format!("{e:?}"))
    );
    for i in 0..12 {
        let w = r.state().waiting_for.clone();
        let res = match &w {
            WaitingFor::DeclareBlockers { .. } => r.declare_blockers(&[(blocker, attacker)]),
            WaitingFor::ReplacementChoice { player, .. } => {
                eprintln!("ReplacementChoice para {player:?}");
                r.act(GameAction::ChooseReplacement { index: 0 })
            }
            WaitingFor::Priority { .. } if r.state().phase == Phase::PostCombatMain => break,
            _ => r.act(GameAction::PassPriority),
        };
        match res {
            Ok(res) => {
                for e in res.events {
                    eprintln!(
                        "    ev {}",
                        format!("{e:?}").chars().take(150).collect::<String>()
                    );
                }
            }
            Err(e) => {
                eprintln!("ERR {e:?}");
                break;
            }
        }
        dump(&format!("c{i}"), r.state());
    }
    eprintln!(
        "bloqueador em {:?}",
        r.state().objects.get(&blocker).map(|o| o.zone)
    );
    (r, attacker, blocker)
}

#[test]
fn combat_one_etching() {
    let r = run_combat(1);
    assert_settled("c1", r.state());
}

/// Relatos a8eb3ab2, b65ef687 e 2b614294: o bloqueador que morreria com DUAS Etching
/// of Kumano tem de ser perguntado (CR 616.1) e ir para o exílio; a partida segue.
#[test]
fn combat_two_etchings() {
    for first_strike in [false, true] {
        let (r, _, blocker) = run_combat_fs(2, first_strike);
        assert_settled("c2", r.state());
        assert!(
            r.state().pending_replacement.is_none(),
            "substituição não fica pendurada"
        );
        assert_eq!(
            r.state().objects.get(&blocker).map(|o| o.zone),
            Some(Zone::Exile),
            "first_strike={first_strike}"
        );
        assert_eq!(r.state().phase, Phase::PostCombatMain, "o combate termina");
    }
}

fn pass_until(r: &mut GameRunner, turn: u32, phase: Phase) {
    for _ in 0..400 {
        if r.state().turn_number == turn && r.state().phase == phase {
            return;
        }
        let w = r.state().waiting_for.clone();
        let res = match w {
            WaitingFor::DeclareAttackers { .. } => r.declare_attackers(&[]),
            WaitingFor::DeclareBlockers { .. } => r.declare_blockers(&[]),
            _ => r.act(GameAction::PassPriority),
        };
        if let Err(e) = res {
            panic!(
                "travou em {:?}/{:?}: {e:?}",
                r.state().phase,
                r.state().waiting_for
            );
        }
    }
    panic!("não chegou ao turno {turn} {phase:?}");
}

/// Relato 2dd1d4bd: Brutal Cathar sozinho — ninguém conjura no próprio turno e ele não transformava.
#[test]
fn brutal_cathar_alone_starts_day_and_turns_night() {
    let db = shared_card_db().expect("card db");
    let mut sc = GameScenario::new();
    sc.at_phase(Phase::PreCombatMain);
    let victim = sc.add_creature(P1, "Opponent Bear", 2, 2).id();
    let cathar = sc.add_real_card(P0, "Brutal Cathar", Zone::Hand, db);
    sc.with_mana_pool(P0, pool(ManaType::White, 3));
    sc.with_library_top(P0, &["Plains", "Plains", "Plains"]);
    sc.with_library_top(P1, &["Mountain", "Mountain", "Mountain"]);
    let mut r = build(sc);
    assert!(r.state().day_night.is_none());
    let _ = r.cast(cathar).commit();
    drive(&mut r, Some(victim), "cathar");
    assert_eq!(
        r.state().day_night,
        Some(engine::types::game_state::DayNight::Day),
        "Cathar entra → dia"
    );
    let t = r.state().turn_number;
    // Turno do oponente sem mágicas → no turno seguinte é noite e o Cathar vira Moonrage Brute.
    pass_until(&mut r, t + 2, Phase::Upkeep);
    assert_eq!(
        r.state().day_night,
        Some(engine::types::game_state::DayNight::Night)
    );
    let obj = &r.state().objects[&cathar];
    assert!(
        obj.transformed,
        "Brutal Cathar transformou (face: {})",
        obj.name
    );
}
