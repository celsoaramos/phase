//! Rodada de relatos 25/11 00h.

use engine::game::scenario::{GameScenario, P0, P1};
use engine::game::triggers::process_triggers;
use engine::game::zone_pipeline::{move_object_for_test, ZoneMoveRequest};
use engine::types::phase::Phase;
use engine::types::zones::Zone;

const SERPENT_SOCIETY: &str = "Deathtouch\nWhenever another creature you control with deathtouch dies, each opponent sacrifices a nontoken creature of their choice.";

/// The Serpent Society: "each opponent sacrifices a nontoken creature of their
/// choice" — the sacrifice is made by the OPPONENT, from the opponent's creatures.
#[test]
fn serpent_society_deathtouch_death_makes_opponent_sacrifice() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.add_creature_from_oracle(P0, "The Serpent Society", 3, 4, SERPENT_SOCIETY);
    let sniper = scenario
        .add_creature_from_oracle(P0, "Dragon Sniper", 1, 1, "Vigilance, reach, deathtouch")
        .id();
    let mine = scenario.add_creature(P0, "Bystander", 2, 2).id();
    let theirs = scenario.add_creature(P1, "Opponent Bear", 2, 2).id();
    let mut runner = scenario.build();

    let mut events = Vec::new();
    assert!(!move_object_for_test(
        runner.state_mut(),
        ZoneMoveRequest::effect(sniper, Zone::Graveyard, sniper),
        &mut events,
    ));
    process_triggers(runner.state_mut(), &events);
    runner.advance_until_stack_empty();

    assert_eq!(
        runner.state().objects[&theirs].zone,
        Zone::Graveyard,
        "the opponent must sacrifice its nontoken creature; waiting_for = {:?}",
        runner.state().waiting_for
    );
    assert_eq!(
        runner.state().objects[&mine].zone,
        Zone::Battlefield,
        "the Serpent Society's controller sacrifices nothing"
    );
}

const BREAKER_OF_ARMIES: &str = "All creatures able to block this creature do so.";

fn breaker_board(
    blockers: usize,
) -> (
    engine::game::scenario::GameRunner,
    engine::types::identifiers::ObjectId,
    Vec<engine::types::identifiers::ObjectId>,
) {
    use engine::game::combat::AttackTarget;
    use engine::types::actions::GameAction;
    use engine::types::game_state::WaitingFor;

    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let breaker = scenario
        .add_creature_from_oracle(P0, "Breaker of Armies", 10, 8, BREAKER_OF_ARMIES)
        .id();
    let others: Vec<_> = (0..4)
        .map(|i| {
            scenario
                .add_creature(P0, &format!("Attacker {i}"), 3, 3)
                .id()
        })
        .collect();
    let goblins: Vec<_> = (0..blockers)
        .map(|i| scenario.add_creature(P1, &format!("Goblin {i}"), 1, 1).id())
        .collect();
    let mut runner = scenario.build();
    for _ in 0..32 {
        match &runner.state().waiting_for {
            WaitingFor::DeclareAttackers { .. } => break,
            _ => runner
                .act(GameAction::PassPriority)
                .expect("pass to attacks"),
        };
    }
    let attacks = std::iter::once(breaker)
        .chain(others)
        .map(|id| (id, AttackTarget::Player(P1)))
        .collect();
    runner
        .act(GameAction::DeclareAttackers {
            attacks,
            bands: vec![],
        })
        .expect("declare attackers");
    for _ in 0..32 {
        match &runner.state().waiting_for {
            WaitingFor::DeclareBlockers { .. } => break,
            _ => runner
                .act(GameAction::PassPriority)
                .expect("pass to blocks"),
        };
    }
    (runner, breaker, goblins)
}

/// Breaker of Armies ("All creatures able to block this creature do so")
/// attacking into a wide board of tokens: the CR 509.1c maximum-requirement
/// solver must answer in bounded time. The table hung with 34+ Goblin tokens
/// blocking (2 s at 38 blockers, 18 s at 60 in a debug build): every blocker
/// contributes one requirement and the empty-first
/// search walked the tie-break tail of the Cartesian product.
#[test]
fn breaker_of_armies_wide_board_declaration_is_bounded() {
    use engine::game::combat::validate_blockers_for_player;
    use engine::types::actions::GameAction;

    let (mut runner, breaker, goblins) = breaker_board(60);
    let started = std::time::Instant::now();
    let all_on_breaker: Vec<_> = goblins.iter().map(|&g| (g, breaker)).collect();
    validate_blockers_for_player(runner.state(), P1, &all_on_breaker)
        .expect("every Goblin blocking Breaker of Armies obeys every requirement");
    let mut one_off = all_on_breaker.clone();
    one_off.pop();
    assert!(
        validate_blockers_for_player(runner.state(), P1, &one_off).is_err(),
        "a Goblin able to block Breaker of Armies must do so"
    );
    runner
        .act(GameAction::DeclareBlockers {
            assignments: all_on_breaker,
        })
        .expect("the full declaration is legal");
    assert!(
        started.elapsed() < std::time::Duration::from_secs(5),
        "the block-requirement solver took {:?}",
        started.elapsed()
    );
}

/// CR 603.10a: a leaves-the-battlefield trigger looks back in time, so The
/// Serpent Society dying in the same event as another deathtouch creature
/// still sees that creature die.
#[test]
fn serpent_society_triggers_when_dying_alongside_deathtouch_creature() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let society = scenario
        .add_creature_from_oracle(P0, "The Serpent Society", 3, 4, SERPENT_SOCIETY)
        .id();
    let sniper = scenario
        .add_creature_from_oracle(P0, "Dragon Sniper", 1, 1, "Vigilance, reach, deathtouch")
        .id();
    let theirs = scenario.add_creature(P1, "Opponent Bear", 2, 2).id();
    let mut runner = scenario.build();

    // Both creatures are dealt lethal damage at once; SBAs destroy them in the
    // same event (CR 704.5g), as a combat where both die.
    for id in [sniper, society] {
        let obj = runner.state_mut().objects.get_mut(&id).unwrap();
        obj.damage_marked = 9;
    }
    let mut events = Vec::new();
    engine::game::sba::check_state_based_actions(runner.state_mut(), &mut events);
    assert_eq!(runner.state().objects[&society].zone, Zone::Graveyard);
    process_triggers(runner.state_mut(), &events);
    runner.advance_until_stack_empty();
    assert_eq!(
        runner.state().objects[&theirs].zone,
        Zone::Graveyard,
        "waiting_for = {:?}",
        runner.state().waiting_for
    );
}
