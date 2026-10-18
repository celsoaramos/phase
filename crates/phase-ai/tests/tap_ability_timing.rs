//! Tap-ability timing and tap-or-untap targeting (MagicFinder report,
//! 2026-10-18).
//!
//! 1. Steel Overseer: the AI tapped it for its counter during its own turn
//!    with nothing else to attack, so it never blocked. The patient line —
//!    activate after the opponent's combat — gives the same counter before the
//!    AI's next turn and keeps the blocker.
//! 2. Sewer-veillance Cam: "tap or untap target creature" aimed at an
//!    opponent's creature that was already tapped (a no-op tap), next to an
//!    untapped one. Whether the AI then ACCEPTS the "you may" is
//!    `OptionalBenefitAcceptPolicy`'s question, tested on its own branch.

use engine::game::scenario::{GameScenario, P0, P1};
use engine::types::ability::TargetRef;
use engine::types::actions::GameAction;
use engine::types::game_state::{GameState, WaitingFor};
use engine::types::identifiers::ObjectId;
use engine::types::keywords::Keyword;
use engine::types::mana::{ManaType, ManaUnit};
use engine::types::phase::Phase;
use engine::types::player::PlayerId;
use phase_ai::choose_action;
use phase_ai::config::{create_config, AiDifficulty, Platform};
use rand::rngs::SmallRng;
use rand::SeedableRng;

const OVERSEER_TEXT: &str = "{T}: Put a +1/+1 counter on each artifact creature you control.";

const DIFFICULTIES: [AiDifficulty; 4] = [
    AiDifficulty::Easy,
    AiDifficulty::Medium,
    AiDifficulty::Hard,
    AiDifficulty::VeryHard,
];

fn choose(state: &GameState, difficulty: AiDifficulty) -> GameAction {
    let config = create_config(difficulty, Platform::Native);
    let mut rng = SmallRng::seed_from_u64(42);
    choose_action(state, P0, &config, &mut rng).expect("AI should return an action")
}

fn activates(action: &GameAction, source: ObjectId) -> bool {
    matches!(action, GameAction::ActivateAbility { source_id, .. } if *source_id == source)
}

fn offered(state: &GameState, source: ObjectId) -> bool {
    let config = create_config(AiDifficulty::VeryHard, Platform::Native);
    phase_ai::score_candidates(state, P0, &config)
        .iter()
        .any(|(action, _)| activates(action, source))
}

/// AI (P0) with a Steel Overseer and a vanilla opponent creature, at `phase`
/// of `active`'s turn with P0 holding priority.
fn overseer_board(
    phase: Phase,
    active: PlayerId,
    extra_artifact_attacker: bool,
) -> (GameState, ObjectId) {
    let mut scenario = GameScenario::new();
    scenario.at_phase(phase);
    let overseer = scenario
        .add_creature_from_oracle(P0, "Steel Overseer", 1, 1, OVERSEER_TEXT)
        .as_artifact_creature()
        .id();
    if extra_artifact_attacker {
        scenario
            .add_creature(P0, "Ornithopter Friend", 2, 2)
            .as_artifact_creature();
    }
    scenario.add_creature(P1, "Opponent Bear", 2, 2);
    let mut runner = scenario.build();
    let state = runner.state_mut();
    state.active_player = active;
    state.priority_player = P0;
    state.waiting_for = WaitingFor::Priority { player: P0 };
    (runner.state().clone(), overseer)
}

#[test]
fn overseer_ability_parses_as_a_tap_only_activation() {
    let (state, overseer) = overseer_board(Phase::End, P1, false);
    assert!(
        offered(&state, overseer),
        "fixture premise: the Overseer's {{T}} ability must be a legal candidate at the opponent's end step"
    );
}

#[test]
fn does_not_tap_overseer_on_own_turn_with_nothing_to_attack() {
    for phase in [
        Phase::Upkeep,
        Phase::Draw,
        Phase::PreCombatMain,
        Phase::BeginCombat,
        Phase::PostCombatMain,
    ] {
        let (state, overseer) = overseer_board(phase, P0, false);
        for difficulty in DIFFICULTIES {
            let action = choose(&state, difficulty);
            assert!(
                !activates(&action, overseer),
                "{difficulty:?} tapped Steel Overseer in its own {phase:?}; it must stay untapped to block"
            );
        }
    }
}

#[test]
fn does_not_tap_overseer_before_the_opponents_combat() {
    for phase in [
        Phase::Upkeep,
        Phase::Draw,
        Phase::PreCombatMain,
        Phase::BeginCombat,
    ] {
        let (state, overseer) = overseer_board(phase, P1, false);
        for difficulty in DIFFICULTIES {
            let action = choose(&state, difficulty);
            assert!(
                !activates(&action, overseer),
                "{difficulty:?} tapped Steel Overseer in the opponent's {phase:?}, giving away the block"
            );
        }
    }
}

#[test]
fn taps_overseer_at_the_opponents_end_step() {
    let (state, overseer) = overseer_board(Phase::End, P1, false);
    for difficulty in DIFFICULTIES {
        let action = choose(&state, difficulty);
        assert!(
            activates(&action, overseer),
            "{difficulty:?} should use the free window (opponent's end step), chose {action:?}"
        );
    }
}

#[test]
fn overseer_stays_available_before_combat_when_it_pumps_an_attacker() {
    // Another artifact creature can attack this turn: the counter buys a
    // bigger attack now, so the hold stands down (the decision is left to the
    // rest of the AI, not forced either way).
    let (state, overseer) = overseer_board(Phase::PreCombatMain, P0, true);
    assert!(
        offered(&state, overseer),
        "the Overseer must still be offered when its counter reaches another attacker"
    );
}

#[test]
fn overseer_is_free_after_blocks_on_the_opponents_turn() {
    let (state, overseer) = overseer_board(Phase::PostCombatMain, P1, false);
    assert!(offered(&state, overseer));
}

// ── Sewer-veillance Cam: tap or untap target creature ─────────────────────

const CAM_TEXT: &str = "Flash\nWhen this artifact enters or leaves the battlefield, you may tap or untap target creature.\n{3}{U}, Sacrifice this artifact: Draw two cards.";

/// Board of the report: the AI (P0) sacrifices the Cam with an opponent
/// creature still tapped from attacking next to an untapped vigilant one.
/// Returns the runner at the Cam's leaves-the-battlefield target prompt.
fn cam_at_target_prompt() -> (engine::game::scenario::GameRunner, ObjectId, ObjectId) {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::Draw);
    let cam = scenario
        .add_artifact_from_oracle(P0, "Sewer-veillance Cam", CAM_TEXT)
        .id();
    scenario
        .add_creature_from_oracle(P0, "Steel Overseer", 1, 1, OVERSEER_TEXT)
        .as_artifact_creature();
    let tapped = scenario
        .add_creature(P1, "Youthful Valkyrie", 2, 2)
        .flying()
        .with_plus_counters(1)
        .id();
    let untapped = scenario
        .add_creature(P1, "Giada", 2, 2)
        .flying()
        .with_keyword(Keyword::Vigilance)
        .id();
    let mut mana = vec![ManaUnit::new(
        ManaType::Blue,
        ObjectId(9_999),
        false,
        vec![],
    )];
    for _ in 0..3 {
        mana.push(ManaUnit::new(
            ManaType::Colorless,
            ObjectId(9_999),
            false,
            vec![],
        ));
    }
    scenario.with_mana_pool(P0, mana);
    let mut runner = scenario.build();
    {
        let state = runner.state_mut();
        state.objects.get_mut(&tapped).unwrap().tapped = true;
        state.active_player = P0;
        state.priority_player = P0;
        state.waiting_for = WaitingFor::Priority { player: P0 };
    }
    let ability_index = runner
        .state()
        .objects
        .get(&cam)
        .unwrap()
        .abilities
        .iter()
        .position(|a| a.cost.is_some())
        .expect("the Cam's sacrifice ability must parse");
    runner
        .act(GameAction::ActivateAbility {
            source_id: cam,
            ability_index,
        })
        .expect("activating the Cam must be legal");
    assert!(
        matches!(
            runner.state().waiting_for,
            WaitingFor::TriggerTargetSelection { player, .. } if player == P0
        ),
        "fixture premise: the leaves-the-battlefield trigger must ask P0 for a target, got {:?}",
        runner.state().waiting_for
    );
    (runner, tapped, untapped)
}

fn target_score(scored: &[(GameAction, f64)], id: ObjectId) -> f64 {
    scored
        .iter()
        .find_map(|(action, score)| match action {
            GameAction::ChooseTarget {
                target: Some(TargetRef::Object(target)),
            } if *target == id => Some(*score),
            _ => None,
        })
        .unwrap_or_else(|| panic!("{id:?} must be a legal target, got {scored:?}"))
}

#[test]
fn tap_or_untap_ranks_an_already_tapped_opponent_creature_below_an_untapped_one() {
    // Before the fix both targets scored the same (-2.194 at every
    // difficulty), so the pick was a coin flip — the report's game landed on
    // the tapped Valkyrie.
    let (runner, tapped, untapped) = cam_at_target_prompt();
    for difficulty in DIFFICULTIES {
        let config = create_config(difficulty, Platform::Native);
        let scored = phase_ai::score_candidates(runner.state(), P0, &config);
        let (t, u) = (
            target_score(&scored, tapped),
            target_score(&scored, untapped),
        );
        assert!(
            t < u - 1.0,
            "{difficulty:?}: tapping an already-tapped opponent creature ({t}) must rank clearly below tapping an untapped one ({u})"
        );
    }
}
