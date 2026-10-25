//! Gideon Blackblade: "Prevent all damage that would be dealt to Gideon
//! Blackblade during your turn."
//!
//! Field report (MagicFinder, 2026-10-25, v0.116.0): an opponent attacked
//! Gideon on THEIR turn and he took zero damage. The prevention parser read
//! the clause up to "~" and dropped the clause-final "during your turn", so
//! the shield prevented damage on every turn.
//!
//! CR 615.1a (prevention shield) + CR 109.5 ("your" = the controller) +
//! CR 102.1 (the active player's turn).

use engine::game::combat::AttackTarget;
use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::types::ability::ReplacementCondition;
use engine::types::actions::GameAction;
use engine::types::game_state::WaitingFor;
use engine::types::identifiers::ObjectId;
use engine::types::phase::Phase;
use engine::types::replacements::ReplacementEvent;

/// Current Oracle text (Scryfall), minus the loyalty abilities.
const GIDEON: &str = "During your turn, Gideon Blackblade is a 4/4 Human Soldier creature with indestructible that's still a planeswalker.\n\
Prevent all damage that would be dealt to Gideon Blackblade during your turn.";

const BOLT: &str = "Test Bolt deals 3 damage to any target.";

struct Board {
    runner: GameRunner,
    gideon: ObjectId,
    bear: ObjectId,
}

fn board() -> Board {
    let mut scenario = GameScenario::new_n_player(2, 7);
    scenario.at_phase(Phase::PreCombatMain);
    for player in [P0, P1] {
        for n in 0..4 {
            scenario.add_card_to_library_top(player, &format!("Filler {n}"));
        }
    }
    let gideon = scenario
        .add_planeswalker_from_oracle(P0, "Gideon Blackblade", "Gideon", 4, GIDEON)
        .id();
    let bear = scenario.add_creature(P1, "Grizzly Bears", 3, 3).id();
    let runner = scenario.build();
    Board {
        runner,
        gideon,
        bear,
    }
}

fn loyalty(runner: &GameRunner, id: ObjectId) -> Option<u32> {
    runner.state().objects.get(&id).and_then(|o| o.loyalty)
}

#[test]
fn gideon_prevention_is_gated_on_its_controllers_turn() {
    let Board { runner, gideon, .. } = board();
    let shields: Vec<_> = runner.state().objects[&gideon]
        .base_replacement_definitions
        .iter()
        .filter(|r| r.event == ReplacementEvent::DamageDone)
        .collect();
    assert_eq!(shields.len(), 1, "one prevention shield");
    assert!(
        matches!(
            shields[0].condition,
            Some(ReplacementCondition::OnlyIfQuantity {
                active_player_req: Some(engine::types::ability::ControllerRef::You),
                ..
            })
        ),
        "the shield applies only during its controller's turn, got {:?}",
        shields[0].condition
    );
}

/// The report: the opponent attacks Gideon on the opponent's own turn.
#[test]
fn gideon_attacked_on_the_opponents_turn_loses_loyalty() {
    let Board {
        mut runner,
        gideon,
        bear,
    } = board();
    runner.state_mut().active_player = P1;
    runner.state_mut().priority_player = P1;
    runner.state_mut().waiting_for = WaitingFor::Priority { player: P1 };
    for _ in 0..24 {
        if runner.waiting_for_kind() == "DeclareAttackers" {
            break;
        }
        runner.act(GameAction::PassPriority).expect("pass");
    }
    runner
        .declare_attackers(&[(bear, AttackTarget::Planeswalker(gideon))])
        .expect("attack Gideon");
    for _ in 0..40 {
        if runner.state().phase == Phase::PostCombatMain {
            break;
        }
        match runner.state().waiting_for.clone() {
            WaitingFor::DeclareBlockers { .. } => {
                runner.declare_blockers(&[]).expect("no blocks");
            }
            WaitingFor::Priority { .. } => {
                runner.act(GameAction::PassPriority).expect("pass");
            }
            other => panic!("unexpected wait in combat: {other:?}"),
        }
    }
    assert_eq!(
        loyalty(&runner, gideon),
        Some(1),
        "3 combat damage on the opponent's turn removes 3 loyalty"
    );
}

/// Noncombat damage on the opponent's turn reaches Gideon too.
#[test]
fn gideon_damage_on_the_opponents_turn_is_not_prevented() {
    let mut scenario = GameScenario::new_n_player(2, 7);
    scenario.at_phase(Phase::PreCombatMain);
    let gideon = scenario
        .add_planeswalker_from_oracle(P0, "Gideon Blackblade", "Gideon", 4, GIDEON)
        .id();
    let bolt = scenario
        .add_spell_to_hand_from_oracle(P1, "Test Bolt", true, BOLT)
        .id();
    let mut runner = scenario.build();
    runner.state_mut().active_player = P1;
    runner.state_mut().priority_player = P1;
    runner.state_mut().waiting_for = WaitingFor::Priority { player: P1 };
    runner.cast(bolt).target_object(gideon).resolve();
    assert_eq!(
        loyalty(&runner, gideon),
        Some(1),
        "not prevented on P1's turn"
    );
}

/// On its controller's turn the shield holds.
#[test]
fn gideon_damage_on_its_controllers_turn_is_prevented() {
    let mut scenario = GameScenario::new_n_player(2, 7);
    scenario.at_phase(Phase::PreCombatMain);
    let gideon = scenario
        .add_planeswalker_from_oracle(P0, "Gideon Blackblade", "Gideon", 4, GIDEON)
        .id();
    let bolt = scenario
        .add_spell_to_hand_from_oracle(P0, "Test Bolt", true, BOLT)
        .id();
    let mut runner = scenario.build();
    runner.cast(bolt).target_object(gideon).resolve();
    assert_eq!(loyalty(&runner, gideon), Some(4), "prevented on P0's turn");
}
