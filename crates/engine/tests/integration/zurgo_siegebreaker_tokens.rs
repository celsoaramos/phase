//! Zurgo, Thunder's Decree + Mardu Siegebreaker (Tarkir: Dragonstorm), relato de campo
//! da mesa online.
//!
//! * Zurgo: "During your end step, Warrior tokens you control have 'This token can't be
//!   sacrificed.'" — a proibição era gravada na PRÓPRIA Zurgo (SelfRef, sem condição), então
//!   os tokens do Mobilize eram sacrificados normalmente.
//! * Siegebreaker: "create a tapped token that's a copy of the exiled card attacking that
//!   opponent" — o sufixo "attacking that opponent" ficava dentro do alvo da cópia
//!   ("a carta exilada" virava um conjunto rastreado sem referente) e a ficha não entrava
//!   atacando; a habilidade de ataque não fazia nada.
//!
//! CR 701.21 (sacrifice), CR 513.1 (end step), CR 607.2a (linked exile), CR 508.4 (enters attacking).

use engine::game::static_abilities::object_has_static_other;
use engine::types::game_state::{ExileLink, ExileLinkKind};

use super::rules::{AttackTarget, GameScenario, ObjectId, Phase, WaitingFor, Zone, P0, P1};

const ZURGO: &str = "Mobilize 2 (Whenever this creature attacks, create two tapped and attacking 1/1 red Warrior creature tokens. Sacrifice them at the beginning of the next end step.)\nDuring your end step, Warrior tokens you control have \"This token can't be sacrificed.\"";

const SIEGEBREAKER: &str = "Deathtouch, haste\nWhen this creature enters, exile up to one other target creature you control until this creature leaves the battlefield.\nWhenever this creature attacks, for each opponent, create a tapped token that's a copy of the exiled card attacking that opponent. At the beginning of your next end step, sacrifice those tokens.";

fn advance_through_combat_to(runner: &mut super::rules::GameRunner, phase: Phase) {
    for _ in 0..40 {
        if runner.state().phase == phase {
            return;
        }
        match &runner.state().waiting_for {
            WaitingFor::Priority { .. } => runner.pass_both_players(),
            WaitingFor::DeclareBlockers { .. } => {
                runner.declare_blockers(&[]).expect("no blockers");
            }
            other => panic!("unexpected window advancing to {phase:?}: {other:?}"),
        }
    }
    panic!("never reached {phase:?}; stuck at {:?}", runner.state().waiting_for);
}

#[test]
fn zurgo_warrior_tokens_survive_the_mobilize_sacrifice() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.with_library_top(P0, &["Filler A"]);
    scenario.with_library_top(P1, &["Filler B"]);
    let zurgo = scenario
        .add_creature_from_oracle(P0, "Zurgo, Thunder's Decree", 2, 4, ZURGO)
        .id();
    let mut runner = scenario.build();

    runner.pass_both_players();
    runner
        .declare_attackers(&[(zurgo, AttackTarget::Player(P1))])
        .expect("declare attackers");
    advance_through_combat_to(&mut runner, Phase::End);
    // Drain the delayed "sacrifice them" trigger at the beginning of the end step.
    for _ in 0..8 {
        if runner.state().stack.is_empty() {
            break;
        }
        runner.pass_both_players();
    }

    let warriors: Vec<ObjectId> = runner
        .state()
        .objects
        .values()
        .filter(|o| o.is_token && o.zone == Zone::Battlefield && o.name == "Warrior")
        .map(|o| o.id)
        .collect();
    assert_eq!(warriors.len(), 2, "both Warrior tokens must survive the end step");
    for id in warriors {
        assert!(object_has_static_other(runner.state(), id, "CantBeSacrificed"));
    }
    // Zurgo itself is not a token: the grant must not be a self-reference.
    assert!(!object_has_static_other(runner.state(), zurgo, "CantBeSacrificed"));
}

#[test]
fn siegebreaker_attack_copies_the_exiled_creature_attacking() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.with_library_top(P0, &["Filler A"]);
    scenario.with_library_top(P1, &["Filler B"]);
    let siege = scenario
        .add_creature_from_oracle(P0, "Mardu Siegebreaker", 4, 4, SIEGEBREAKER)
        .id();
    let bear = scenario.add_creature(P0, "Grizzly Bears", 2, 2).id();
    let mut runner = scenario.build();

    // Simulate the ETB having exiled the bear "until Siegebreaker leaves".
    {
        let state = runner.state_mut();
        engine::game::zones::move_to_zone(state, bear, Zone::Exile, &mut Vec::new());
        state.exile_links.push(ExileLink {
            exiled_id: bear,
            source_id: siege,
            kind: ExileLinkKind::UntilSourceLeaves {
                return_zone: Zone::Battlefield,
            },
        });
    }

    runner.pass_both_players();
    runner
        .declare_attackers(&[(siege, AttackTarget::Player(P1))])
        .expect("declare attackers");
    // Resolve the attack trigger.
    for _ in 0..6 {
        if runner.state().stack.is_empty() {
            break;
        }
        runner.pass_both_players();
    }

    let copies: Vec<_> = runner
        .state()
        .objects
        .values()
        .filter(|o| o.is_token && o.zone == Zone::Battlefield && o.name == "Grizzly Bears")
        .map(|o| o.id)
        .collect();
    assert_eq!(copies.len(), 1, "one token copy of the exiled creature");
    let combat = runner.state().combat.as_ref().expect("in combat");
    assert!(
        combat.attackers.iter().any(|a| a.object_id == copies[0]),
        "the copy must enter attacking"
    );
}
