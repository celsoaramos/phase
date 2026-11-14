//! Star Athlete: "Whenever this creature attacks, choose up to one target
//! nonland permanent. Its controller may sacrifice it. If they don't, this
//! creature deals 5 damage to that player."
//!
//! Field report (MagicFinder, 2026-10-25, v0.116.0): the defending player
//! declined to sacrifice the chosen permanent and took no damage.
//! CR 608.2c + CR 608.2d + CR 109.4.

use engine::game::combat::AttackTarget;
use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::types::actions::GameAction;
use engine::types::game_state::WaitingFor;
use engine::types::identifiers::ObjectId;
use engine::types::phase::Phase;

const STAR_ATHLETE: &str = "Menace\nWhenever this creature attacks, choose up to one target nonland permanent. Its controller may sacrifice it. If they don't, this creature deals 5 damage to that player.";

fn advance_to_declare_attackers(runner: &mut GameRunner) {
    runner.state_mut().active_player = P0;
    runner.state_mut().priority_player = P0;
    runner.state_mut().waiting_for = WaitingFor::Priority { player: P0 };
    for _ in 0..24 {
        if runner.waiting_for_kind() == "DeclareAttackers" {
            return;
        }
        runner.act(GameAction::PassPriority).expect("pass");
    }
    panic!(
        "did not reach DeclareAttackers: {:?}",
        runner.state().waiting_for
    );
}

/// Drive the attack trigger to completion: target `chosen`, then answer the
/// optional sacrifice with `accept`. Returns who was asked the optional choice.
fn resolve_attack_trigger(
    runner: &mut GameRunner,
    chosen: ObjectId,
    accept: bool,
) -> Vec<engine::types::player::PlayerId> {
    let mut asked = Vec::new();
    for _ in 0..60 {
        match runner.state().waiting_for.clone() {
            WaitingFor::TriggerTargetSelection { selection, .. } => {
                let target = selection
                    .current_legal_targets
                    .iter()
                    .find(|t| **t == engine::types::ability::TargetRef::Object(chosen))
                    .cloned();
                assert!(target.is_some(), "the nonland permanent must be targetable");
                runner
                    .act(GameAction::ChooseTarget { target })
                    .expect("choose target");
            }
            WaitingFor::OptionalEffectChoice { player, .. } => {
                asked.push(player);
                runner
                    .act(GameAction::DecideOptionalEffect { accept })
                    .expect("optional decision");
            }
            WaitingFor::Priority { .. } => {
                if runner.state().stack.is_empty() {
                    return asked;
                }
                runner.act(GameAction::PassPriority).expect("pass");
            }
            other => panic!("unexpected wait while resolving the trigger: {other:?}"),
        }
    }
    panic!("attack trigger never finished resolving");
}

fn setup() -> (GameRunner, ObjectId, ObjectId) {
    let mut scenario = GameScenario::new_n_player(2, 7);
    scenario.at_phase(Phase::PreCombatMain);
    let athlete = scenario
        .add_creature_from_oracle(P0, "Star Athlete", 3, 2, STAR_ATHLETE)
        .id();
    let tribunal = scenario
        .add_enchantment_from_oracle(P1, "Test Enchantment", "")
        .id();
    let mut runner = scenario.build();
    advance_to_declare_attackers(&mut runner);
    runner
        .declare_attackers(&[(athlete, AttackTarget::Player(P1))])
        .expect("declare Star Athlete");
    (runner, athlete, tribunal)
}

#[test]
fn star_athlete_declined_sacrifice_deals_five_to_the_permanents_controller() {
    let (mut runner, _athlete, tribunal) = setup();
    let life_before = runner.state().players[1].life;
    let asked = resolve_attack_trigger(&mut runner, tribunal, false);

    assert_eq!(
        asked,
        vec![P1],
        "the permanent's controller decides the sacrifice"
    );
    assert!(
        runner.state().battlefield.contains(&tribunal),
        "declined: the permanent stays"
    );
    assert_eq!(
        runner.state().players[1].life,
        life_before - 5,
        "declined: Star Athlete deals 5 damage to that player"
    );
    assert_eq!(runner.state().players[0].life, 20, "never the attacker");
}

#[test]
fn star_athlete_accepted_sacrifice_deals_no_damage() {
    let (mut runner, _athlete, tribunal) = setup();
    let life_before = runner.state().players[1].life;
    resolve_attack_trigger(&mut runner, tribunal, true);

    assert!(
        !runner.state().battlefield.contains(&tribunal),
        "accepted: the permanent is sacrificed"
    );
    assert_eq!(
        runner.state().players[1].life,
        life_before,
        "accepted: no damage"
    );
}

/// The may belongs to the chosen permanent's controller: the parser records it
/// on the Sacrifice node, which has no player slot of its own.
#[test]
fn star_athlete_sacrifice_may_is_announced_by_the_permanents_controller() {
    use engine::types::ability::{Effect, TargetFilter};
    let parsed = engine::parser::parse_oracle_text(
        STAR_ATHLETE,
        "Star Athlete",
        &[],
        &["Creature".to_string()],
        &[],
    );
    let execute = parsed.triggers[0]
        .execute
        .as_ref()
        .expect("attack trigger body");
    let sacrifice = execute.sub_ability.as_ref().expect("the sacrifice clause");
    assert!(matches!(&*sacrifice.effect, Effect::Sacrifice { .. }));
    assert!(sacrifice.optional);
    assert_eq!(
        sacrifice.optional_player,
        Some(TargetFilter::ParentTargetController)
    );
}
