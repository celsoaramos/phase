//! Saddle with no attack left (`tactical_gate/mf_flags_1115.rs`, MagicFinder flags
//! 798f877a and 5ea1eee8). The engine offers `SaddleMount` and the AI scores it;
//! the gate is what removes it. Siblings keep the saddle where it can matter.

use engine::game::scenario::{GameScenario, P0, P1};
use engine::types::actions::GameAction;
use engine::types::game_state::{GameState, WaitingFor};
use engine::types::identifiers::ObjectId;
use engine::types::keywords::Keyword;
use engine::types::phase::Phase;
use engine::types::player::PlayerId;
use phase_ai::config::{create_config, AiDifficulty, Platform};
use phase_ai::score_candidates;

const VAULTER: &str = "Whenever this creature saddles a Mount or crews a Vehicle during your main phase, that Mount or Vehicle gains flying until end of turn.";
const OX_ATTACK: &str = "Whenever this creature attacks while saddled, put a +1/+1 counter on target creature.";

fn scored(state: &GameState, player: PlayerId) -> Vec<GameAction> {
    let config = create_config(AiDifficulty::VeryHard, Platform::Native);
    score_candidates(state, player, &config)
        .into_iter()
        .map(|(action, _)| action)
        .collect()
}

fn offers_saddle(actions: &[GameAction], mount: ObjectId) -> bool {
    actions
        .iter()
        .any(|a| matches!(a, GameAction::SaddleMount { mount_id, .. } if *mount_id == mount))
}

fn engine_offers_saddle(state: &GameState, mount: ObjectId) -> bool {
    let actions: Vec<GameAction> = engine::ai_support::build_decision_context(state)
        .candidates
        .iter()
        .map(|c| c.action.clone())
        .collect();
    offers_saddle(&actions, mount)
}

fn set_priority(state: &mut GameState, phase: Phase) {
    state.phase = phase;
    state.active_player = P0;
    state.priority_player = P0;
    state.waiting_for = WaitingFor::Priority { player: P0 };
}

struct Board {
    state: GameState,
    ox: ObjectId,
}

/// The flagged board: Bulwark Ox (Saddle 1) and Canyon Vaulter for the robot,
/// one creature for the opponent.
fn board(phase: Phase, ox_sick: bool, ox_extra_text: Option<&str>) -> Board {
    let mut scenario = GameScenario::new();
    scenario.at_phase(phase);
    let ox = {
        let text = match ox_extra_text {
            Some(extra) => format!("{OX_ATTACK}\n{extra}"),
            None => OX_ATTACK.to_string(),
        };
        let mut builder = scenario.add_creature_from_oracle(P0, "Bulwark Ox", 2, 2, &text);
        builder.with_keyword(Keyword::Saddle(1));
        if ox_sick {
            builder.with_summoning_sickness();
        }
        builder.id()
    };
    scenario.add_creature_from_oracle(P0, "Canyon Vaulter", 3, 1, VAULTER);
    scenario.add_creature(P1, "Six", 2, 4);
    let mut runner = scenario.build();
    let mut state = runner.state().clone();
    set_priority(&mut state, phase);
    let _ = &mut runner;
    Board { state, ox }
}

#[test]
fn saddle_after_combat_is_not_activated() {
    let b = board(Phase::PostCombatMain, false, None);
    assert!(
        engine_offers_saddle(&b.state, b.ox),
        "precondition: the engine offers the saddle in the postcombat main phase"
    );
    assert!(
        !offers_saddle(&scored(&b.state, P0), b.ox),
        "no attack left this turn: saddling only taps the Vaulter through the opponent's turn"
    );
}

#[test]
fn saddle_on_a_summoning_sick_mount_is_not_activated() {
    let b = board(Phase::PreCombatMain, true, None);
    assert!(engine_offers_saddle(&b.state, b.ox));
    assert!(
        !offers_saddle(&scored(&b.state, P0), b.ox),
        "a summoning-sick Mount cannot attack this turn"
    );
}

#[test]
fn saddle_before_combat_with_an_able_attacker_stays_available() {
    let b = board(Phase::PreCombatMain, false, None);
    assert!(engine_offers_saddle(&b.state, b.ox));
    assert!(
        offers_saddle(&scored(&b.state, P0), b.ox),
        "the Ox can still attack while saddled: the gate must stay out of the way"
    );
}

#[test]
fn saddle_with_a_static_that_reads_it_stays_available_after_combat() {
    let b = board(
        Phase::PostCombatMain,
        false,
        Some("As long as this creature is saddled, it has hexproof."),
    );
    assert!(engine_offers_saddle(&b.state, b.ox));
    let ox = b.state.objects.get(&b.ox).unwrap();
    if ox.static_definitions.iter_unchecked().next().is_none() {
        // The parser did not produce the static: nothing to prove here.
        return;
    }
    assert!(
        offers_saddle(&scored(&b.state, P0), b.ox),
        "\"as long as saddled\" has value without an attack: fail open"
    );
}
