//! Soul-Scar Mage — "If a source you control would deal noncombat damage to a
//! creature an opponent controls, put that many -1/-1 counters on that creature
//! instead."
//!
//! CR 614.1a + CR 614.6: a REPLACEMENT (not a CR 615 prevention). The damage
//! event never happens — no marked damage, no `DamageDealt`, no
//! `DamagePrevented` — and the counters are put on the would-be recipient.
//! Only noncombat damage from sources its controller controls, and only to
//! creatures an opponent controls, is replaced.
//!
//! Prowess is omitted from the fixture Oracle text: its trigger on casting the
//! test's instant is irrelevant to the replacement under test.

use engine::game::combat::AttackTarget;
use engine::game::scenario::{GameScenario, P0, P1};
use engine::types::actions::GameAction;
use engine::types::counter::CounterType;
use engine::types::events::GameEvent;
use engine::types::game_state::WaitingFor;
use engine::types::identifiers::ObjectId;
use engine::types::mana::{ManaType, ManaUnit};
use engine::types::phase::Phase;

const SOUL_SCAR_ORACLE: &str = "If a source you control would deal noncombat damage to a \
creature an opponent controls, put that many -1/-1 counters on that creature instead.";

fn red_mana() -> Vec<ManaUnit> {
    vec![ManaUnit::new(ManaType::Red, ObjectId(0), false, vec![])]
}

fn give_priority(
    runner: &mut engine::game::scenario::GameRunner,
    player: engine::types::player::PlayerId,
) {
    let state = runner.state_mut();
    state.active_player = player;
    state.priority_player = player;
    state.waiting_for = WaitingFor::Priority { player };
}

/// CR 614.6 + CR 704.5q: your burn spell at an opponent's creature puts that
/// many -1/-1 counters on it instead of dealing damage; +1/+1 counters already
/// on it annihilate with the new -1/-1 counters.
#[test]
fn burn_at_opponents_creature_becomes_minus_counters() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.add_creature_from_oracle(P0, "Soul-Scar Mage", 1, 2, SOUL_SCAR_ORACLE);
    let ogre = scenario
        .add_creature(P1, "Hill Ogre", 4, 4)
        .with_plus_counters(1)
        .id();
    let bolt = scenario.add_bolt_to_hand(P0);
    scenario.with_mana_pool(P0, red_mana());

    let mut runner = scenario.build();
    give_priority(&mut runner, P0);
    let outcome = runner.cast(bolt).target_object(ogre).resolve();

    assert_eq!(
        outcome.damage_marked(ogre),
        0,
        "replaced damage is never dealt"
    );
    // 3 -1/-1 counters arrive; one annihilates with the +1/+1 counter.
    outcome.assert_counters(ogre, CounterType::Plus1Plus1, 0);
    outcome.assert_counters(ogre, CounterType::Minus1Minus1, 2);
    assert_eq!(outcome.power_toughness(ogre), (2, 2));
    assert!(
        !outcome
            .events()
            .iter()
            .any(|e| matches!(e, GameEvent::DamagePrevented { .. })),
        "a replacement is not a prevention: no DamagePrevented"
    );
    assert!(
        !outcome.events().iter().any(|e| matches!(
            e,
            GameEvent::DamageDealt { target, .. }
                if *target == engine::types::ability::TargetRef::Object(ogre)
        )),
        "no damage is dealt to the creature"
    );
}

/// CR 614.1a: the recipient scope is "a creature an opponent controls" — a
/// player is not replaced.
#[test]
fn burn_at_opponent_player_deals_normal_damage() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.add_creature_from_oracle(P0, "Soul-Scar Mage", 1, 2, SOUL_SCAR_ORACLE);
    let bolt = scenario.add_bolt_to_hand(P0);
    scenario.with_mana_pool(P0, red_mana());

    let mut runner = scenario.build();
    give_priority(&mut runner, P0);
    let outcome = runner.cast(bolt).target_player(P1).resolve();

    assert_eq!(outcome.life_delta(P1), -3);
}

/// CR 614.1a: your own creature is not "a creature an opponent controls".
#[test]
fn burn_at_own_creature_deals_normal_damage() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.add_creature_from_oracle(P0, "Soul-Scar Mage", 1, 2, SOUL_SCAR_ORACLE);
    let golem = scenario.add_creature(P0, "Own Golem", 4, 4).id();
    let bolt = scenario.add_bolt_to_hand(P0);
    scenario.with_mana_pool(P0, red_mana());

    let mut runner = scenario.build();
    give_priority(&mut runner, P0);
    let outcome = runner.cast(bolt).target_object(golem).resolve();

    assert_eq!(outcome.damage_marked(golem), 3);
    outcome.assert_counters(golem, CounterType::Minus1Minus1, 0);
}

/// CR 614.1a: only sources Soul-Scar's controller controls are replaced — an
/// opponent's burn spell at your creature deals damage normally.
#[test]
fn opponents_burn_at_your_creature_deals_normal_damage() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.add_creature_from_oracle(P0, "Soul-Scar Mage", 1, 2, SOUL_SCAR_ORACLE);
    let golem = scenario.add_creature(P0, "Own Golem", 4, 4).id();
    let bolt = scenario.add_bolt_to_hand(P1);
    scenario.with_mana_pool(P1, red_mana());

    let mut runner = scenario.build();
    give_priority(&mut runner, P1);
    let outcome = runner.cast(bolt).target_object(golem).resolve();

    assert_eq!(outcome.damage_marked(golem), 3);
    outcome.assert_counters(golem, CounterType::Minus1Minus1, 0);
}

/// CR 120.2a + CR 510.2: combat damage is not noncombat damage — an attacker
/// you control blocked by an opponent's creature deals ordinary damage.
#[test]
fn combat_damage_is_not_replaced() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.add_creature_from_oracle(P0, "Soul-Scar Mage", 1, 2, SOUL_SCAR_ORACLE);
    let attacker = scenario.add_creature(P0, "Attacker", 3, 3).id();
    let blocker = scenario.add_creature(P1, "Blocker", 1, 5).id();

    let mut runner = scenario.build();
    runner.advance_to_combat();
    runner
        .declare_attackers(&[(attacker, AttackTarget::Player(P1))])
        .expect("declare attacker");
    for _ in 0..8 {
        if matches!(
            runner.state().waiting_for,
            WaitingFor::DeclareBlockers { .. }
        ) {
            break;
        }
        runner
            .act(GameAction::PassPriority)
            .expect("advance to blockers");
    }
    runner
        .declare_blockers(&[(blocker, attacker)])
        .expect("declare blocker");
    let outcome = runner.combat_damage();

    assert_eq!(outcome.damage_marked(blocker), 3);
    outcome.assert_counters(blocker, CounterType::Minus1Minus1, 0);
}
