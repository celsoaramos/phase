//! Relato da mesa (11/11/2026): Pack Leader atacando com dezenas de Gatos —
//! "Player 2 is dealt 0 combat damage by 43 creatures". Pack Leader's
//! prevention is "to Dogs you control"; damage to the defending player and to
//! non-Dog creatures must still be dealt.

use engine::game::combat::AttackTarget;
use engine::game::scenario::GameScenario;
use engine::types::phase::Phase;
use engine::types::player::PlayerId;

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

const PACK_LEADER: &str = "Other Dogs you control get +1/+1.\nWhenever Pack Leader attacks, prevent all combat damage that would be dealt this turn to Dogs you control.";

#[test]
fn pack_leader_prevention_does_not_shield_the_defending_player() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let leader = scenario
        .add_creature_from_oracle(P0, "Pack Leader", 2, 2, PACK_LEADER)
        .with_subtypes(vec!["Dog"])
        .id();
    let cat = scenario
        .add_creature(P0, "Cat", 2, 2)
        .with_subtypes(vec!["Cat"])
        .id();
    let mut runner = scenario.build();
    let life_before = runner.state().players[1].life;

    runner.advance_to_combat();
    runner
        .declare_attackers(&[
            (leader, AttackTarget::Player(P1)),
            (cat, AttackTarget::Player(P1)),
        ])
        .expect("both attack");
    runner.advance_until_stack_empty();
    runner.combat_damage();

    assert_eq!(
        runner.state().players[1].life,
        life_before - 4,
        "Pack Leader prevents damage dealt TO Dogs, not damage dealt BY attackers to the player"
    );
}

#[test]
fn pack_leader_prevention_shields_only_dogs_you_control() {
    use engine::game::engine::apply_as_current;
    use engine::types::actions::GameAction;
    use engine::types::game_state::WaitingFor;

    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let leader = scenario
        .add_creature_from_oracle(P0, "Pack Leader", 2, 2, PACK_LEADER)
        .with_subtypes(vec!["Dog"])
        .id();
    let cat = scenario
        .add_creature(P0, "Cat", 2, 4)
        .with_subtypes(vec!["Cat"])
        .id();
    let wall_a = scenario.add_creature(P1, "Bear A", 3, 3).id();
    let wall_b = scenario.add_creature(P1, "Bear B", 3, 3).id();
    let mut runner = scenario.build();

    runner.advance_to_combat();
    runner
        .declare_attackers(&[
            (leader, AttackTarget::Player(P1)),
            (cat, AttackTarget::Player(P1)),
        ])
        .expect("both attack");
    for _ in 0..10 {
        if matches!(
            runner.state().waiting_for,
            WaitingFor::DeclareBlockers { .. }
        ) {
            break;
        }
        let _ = apply_as_current(runner.state_mut(), GameAction::PassPriority);
    }
    runner
        .declare_blockers(&[(wall_a, leader), (wall_b, cat)])
        .expect("blocks");
    runner.combat_damage();

    assert_eq!(
        runner.state().objects[&leader].damage_marked,
        0,
        "combat damage to a Dog you control is prevented"
    );
    assert_eq!(
        runner.state().objects[&cat].damage_marked,
        3,
        "a non-Dog attacker is not shielded"
    );
}
