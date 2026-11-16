//! Temporada 2026-12 da Mesa do MagicFinder — o comandante Jace, Multiverse
//! Architect saía com a lacuna `unrecognized_clause_head`:
//! "At the beginning of combat on each opponent's turn, they may pay {2}. If they
//! don't, creatures they control can't attack Jaces you control this turn."
//!
//! Duas peças: o escopo defendido "<tipo de planeswalker>s you control" virou
//! `AttackTargetFilter::PlaneswalkerWithSubtype` (CR 508.1c + CR 205.3j), e o
//! construtor de restrição aceita o sujeito "creatures they control" para esse
//! escopo (forma de concessão congelada — desvio conhecido de CR 611.2c: criatura
//! que chega depois, no mesmo turno, não fica coberta).

use engine::game::combat::AttackTarget;
use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::parser::oracle::parse_oracle_text;
use engine::types::ability::Effect;
use engine::types::actions::GameAction;
use engine::types::game_state::WaitingFor;
use engine::types::identifiers::ObjectId;
use engine::types::phase::Phase;

const JACE_TRIGGER: &str = "At the beginning of combat on each opponent's turn, they may pay \
{2}. If they don't, creatures they control can't attack Jaces you control this turn.";

#[test]
fn the_decline_branch_parses_as_a_jace_scoped_attack_restriction() {
    let parsed = parse_oracle_text(
        JACE_TRIGGER,
        "Jace, Multiverse Architect",
        &[],
        &["Planeswalker".to_string()],
        &["Jace".to_string()],
    );
    let exec = parsed.triggers[0].execute.as_deref().unwrap();
    let sub = exec.sub_ability.as_deref().expect("the decline branch");
    assert!(
        matches!(&*sub.effect, Effect::GenericEffect { .. }),
        "the restriction is represented: {:?}",
        sub.effect
    );
    let text = format!("{:?}", sub.effect);
    assert!(
        text.contains("PlaneswalkerWithSubtype") && text.contains("Jace"),
        "{text}"
    );
}

/// "Goblins you control" is not a planeswalker scope the restriction claims for a
/// broadcast subject unless it names a type: the honest gap for the plain
/// broadcast forms stays (Chronomantic Escape shape).
#[test]
fn broadcast_can_t_attack_you_keeps_its_gap() {
    let parsed = parse_oracle_text(
        "Until your next turn, creatures can't attack you or planeswalkers you control.",
        "Probe",
        &[],
        &["Sorcery".to_string()],
        &[],
    );
    let text = format!("{:?}", parsed.abilities);
    assert!(text.contains("Unimplemented"), "{text}");
}

struct Board {
    runner: GameRunner,
    jace: ObjectId,
    other_walker: ObjectId,
    attacker: ObjectId,
}

fn board() -> Board {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let jace = scenario
        .add_planeswalker_from_oracle(P0, "Jace, Multiverse Architect", "Jace", 5, JACE_TRIGGER)
        .id();
    let other_walker = scenario
        .add_planeswalker_from_oracle(P0, "Chandra, Test Walker", "Chandra", 5, "")
        .id();
    let attacker = scenario.add_creature(P1, "Grizzly Bears", 2, 2).id();
    let mut runner = scenario.build();
    runner.state_mut().active_player = P1;
    Board {
        runner,
        jace,
        other_walker,
        attacker,
    }
}

/// Drive P1's turn to declare-attackers, declining the {2} when asked.
fn reach_declare_attackers(runner: &mut GameRunner) -> bool {
    let mut asked = false;
    for _ in 0..24 {
        match runner.state().waiting_for.clone() {
            WaitingFor::DeclareAttackers { .. } => return asked,
            WaitingFor::OptionalEffectChoice { .. } | WaitingFor::OptionalCostChoice { .. } => {
                asked = true;
                runner
                    .act(GameAction::DecideOptionalEffect { accept: false })
                    .or_else(|_| runner.act(GameAction::DecideOptionalCost { pay: false }))
                    .expect("declining the {2} is legal");
            }
            WaitingFor::UnlessPayment { .. } => {
                asked = true;
                runner
                    .act(GameAction::DecideOptionalEffect { accept: false })
                    .expect("declining the {2} is legal");
            }
            WaitingFor::Priority { .. } => {
                runner.act(GameAction::PassPriority).expect("pass");
            }
            other => panic!("unexpected state on the way to combat: {other:?}"),
        }
    }
    panic!("never reached declare attackers")
}

#[test]
fn declining_the_two_forbids_attacking_jace_only() {
    let Board {
        mut runner,
        jace,
        other_walker,
        attacker,
    } = board();
    assert!(
        reach_declare_attackers(&mut runner),
        "the opponent is asked to pay {{2}} at the beginning of combat"
    );
    assert!(
        runner
            .declare_attackers(&[(attacker, AttackTarget::Planeswalker(jace))])
            .is_err(),
        "creatures they control can't attack Jaces you control"
    );
    assert!(
        runner
            .declare_attackers(&[(attacker, AttackTarget::Planeswalker(other_walker))])
            .is_ok(),
        "a non-Jace planeswalker can still be attacked"
    );
}

#[test]
fn the_player_can_still_be_attacked_after_declining() {
    let Board {
        mut runner,
        attacker,
        ..
    } = board();
    reach_declare_attackers(&mut runner);
    assert!(runner
        .declare_attackers(&[(attacker, AttackTarget::Player(P0))])
        .is_ok());
}
