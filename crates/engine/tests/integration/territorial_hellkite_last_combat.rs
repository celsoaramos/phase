//! Territorial Hellkite — "At the beginning of combat on your turn, choose an
//! opponent at random that Territorial Hellkite didn't attack during your last
//! combat. Territorial Hellkite attacks that player this combat if able. If you
//! can't choose an opponent this way, tap Territorial Hellkite."
//!
//! Field report: the Hellkite turned sideways at every beginning of combat and
//! never dealt damage. Two parse losses compounded:
//! - "that ~ didn't attack during your last combat" was dropped, so the choice
//!   ranged over every opponent every turn;
//! - "If you can't choose an opponent this way" became the generic "if you
//!   can't" gate (`Not(ZoneChangedThisWay)`), which is TRUE after a choice —
//!   nothing changes zone — so the Hellkite was tapped right after being told to
//!   attack, and a tapped creature can't be declared as an attacker (CR 508.1a).
//!
//! Claim-to-test matrix (1v1, real turn flow):
//! - first combat → it is NOT tapped, must attack, and its 6 damage lands;
//! - the controller's next combat → the only opponent was attacked during the
//!   last combat, no opponent can be chosen, so it is tapped and deals nothing;
//! - the combat after that → the last combat had no attack by it, so it attacks
//!   again (the printed alternation in two-player games);
//! - the parse carries the restriction and the "no player chosen" gate.

use engine::game::combat::{creature_must_attack, AttackTarget};
use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::parser::oracle::parse_oracle_text;
use engine::types::ability::{
    AbilityCondition, AttackSubject, ChoiceType, CombatHistoryScope, Effect, PlayerFilter,
};
use engine::types::actions::GameAction;
use engine::types::game_state::WaitingFor;
use engine::types::identifiers::ObjectId;
use engine::types::phase::Phase;
use engine::types::player::PlayerId;

const HELLKITE: &str = "At the beginning of combat on your turn, choose an opponent at random \
     that Territorial Hellkite didn't attack during your last combat. Territorial Hellkite \
     attacks that player this combat if able. If you can't choose an opponent this way, tap \
     Territorial Hellkite.";

fn life(runner: &GameRunner, player: PlayerId) -> i32 {
    runner
        .state()
        .players
        .iter()
        .find(|p| p.id == player)
        .expect("player exists")
        .life
}

fn tapped(runner: &GameRunner, id: ObjectId) -> bool {
    runner
        .state()
        .objects
        .get(&id)
        .expect("on battlefield")
        .tapped
}

/// Walks the real turn structure until `player` holds priority in `phase`,
/// declaring no attackers in any combat met on the way.
fn advance_to(runner: &mut GameRunner, player: PlayerId, phase: Phase) {
    for _ in 0..200 {
        let state = runner.state();
        if state.active_player == player
            && state.phase == phase
            && matches!(state.waiting_for, WaitingFor::Priority { .. })
            && state.stack.is_empty()
        {
            return;
        }
        let action = match state.waiting_for {
            WaitingFor::DeclareAttackers { .. } => GameAction::DeclareAttackers {
                attacks: vec![],
                bands: vec![],
            },
            WaitingFor::DeclareBlockers { .. } => GameAction::DeclareBlockers {
                assignments: vec![],
            },
            _ => GameAction::PassPriority,
        };
        runner.act(action).unwrap_or_else(|e| {
            panic!(
                "could not advance: {e:?} at {:?}",
                runner.state().waiting_for
            )
        });
    }
    panic!("never reached {phase:?} of {player:?}");
}

/// From the active player's precombat main: pass into beginning of combat and
/// resolve the Hellkite trigger. Returns with the trigger resolved.
fn begin_combat(runner: &mut GameRunner) {
    runner.pass_both_players();
    assert_eq!(runner.state().phase, Phase::BeginCombat);
    assert!(
        !runner.state().stack.is_empty(),
        "the beginning-of-combat trigger must reach the stack"
    );
    runner.advance_until_stack_empty();
}

/// Beginning of combat → (declare the Hellkite if it can) → end of combat.
/// Returns whether the engine allowed and required the attack.
fn run_combat(runner: &mut GameRunner, hellkite: ObjectId) -> bool {
    begin_combat(runner);
    let forced = !tapped(runner, hellkite) && creature_must_attack(runner.state(), hellkite);
    if forced {
        runner
            .declare_attackers(&[(hellkite, AttackTarget::Player(P1))])
            .expect("the Hellkite attacks the chosen opponent");
    }
    advance_to(runner, P0, Phase::PostCombatMain);
    forced
}

fn scenario() -> (GameRunner, ObjectId) {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.with_library_top(P0, &["Forest", "Forest", "Forest", "Forest"]);
    scenario.with_library_top(P1, &["Forest", "Forest", "Forest", "Forest"]);
    let hellkite = {
        let mut builder =
            scenario.add_creature_from_oracle(P0, "Territorial Hellkite", 6, 5, HELLKITE);
        builder.flying().haste();
        builder.id()
    };
    (scenario.build(), hellkite)
}

/// CR 508.1d + CR 506.1: on its first combat the Hellkite attacked no one during
/// your last combat, so the only opponent is chosen, it stays untapped, is
/// required to attack, and the damage lands.
#[test]
fn territorial_hellkite_attacks_on_its_first_combat() {
    let (mut runner, hellkite) = scenario();

    assert!(
        run_combat(&mut runner, hellkite),
        "first combat: forced to attack, untapped"
    );
    assert_eq!(life(&runner, P1), 14, "the 6 combat damage must land");
}

/// CR 608.2d + CR 506.1: in a two-player game the next combat has no opponent
/// left to choose — it attacked the only one during your last combat — so it is
/// tapped and deals nothing; the combat after that it attacks again.
#[test]
fn territorial_hellkite_alternates_attack_and_tap_in_two_player() {
    let (mut runner, hellkite) = scenario();

    assert!(run_combat(&mut runner, hellkite), "turn A: attacks");
    assert_eq!(life(&runner, P1), 14);

    advance_to(&mut runner, P0, Phase::PreCombatMain);
    begin_combat(&mut runner);
    assert!(
        tapped(&runner, hellkite),
        "turn B: no opponent can be chosen this way, so the Hellkite is tapped"
    );
    advance_to(&mut runner, P0, Phase::PostCombatMain);
    assert_eq!(life(&runner, P1), 14, "turn B: no damage");

    advance_to(&mut runner, P0, Phase::PreCombatMain);
    assert!(
        run_combat(&mut runner, hellkite),
        "turn C: it didn't attack during your last combat, so it attacks again"
    );
    assert_eq!(life(&runner, P1), 8);
}

/// The parse keeps both halves the field report lost.
#[test]
fn territorial_hellkite_parses_restriction_and_no_choice_gate() {
    let parsed = parse_oracle_text(
        &format!("Flying, haste\n{HELLKITE}"),
        "Territorial Hellkite",
        &[],
        &["Creature".to_string()],
        &["Dragon".to_string()],
    );
    let execute = parsed.triggers[0].execute.as_ref().expect("execute chain");
    let Effect::Choose { choice_type, .. } = &*execute.effect else {
        panic!(
            "parent must be the opponent choice, got {:?}",
            execute.effect
        );
    };
    assert_eq!(
        *choice_type,
        ChoiceType::Opponent {
            restriction: Some(Box::new(PlayerFilter::AllExcept {
                exclude: Box::new(PlayerFilter::OpponentAttacked {
                    subject: AttackSubject::Source,
                    scope: CombatHistoryScope::LastCombat,
                }),
            })),
            distinctness: Default::default(),
        }
    );
    let force_attack = execute.sub_ability.as_ref().expect("force attack");
    assert!(matches!(&*force_attack.effect, Effect::ForceAttack { .. }));
    let tap = force_attack.sub_ability.as_ref().expect("tap rider");
    assert_eq!(tap.condition, Some(AbilityCondition::NoPlayerChosen));
}
