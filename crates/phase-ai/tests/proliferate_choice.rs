//! MagicFinder 05/11 (Metastatic Evangel): the AI proliferated its opponent's
//! energy. These tests resolve a real proliferate and ask the AI to choose.
//! CR 701.34a.

use engine::game::scenario::{GameScenario, P0, P1};
use engine::types::ability::TargetRef;
use engine::types::actions::GameAction;
use engine::types::game_state::WaitingFor;
use engine::types::identifiers::ObjectId;
use engine::types::phase::Phase;
use phase_ai::choose_action;
use phase_ai::config::{create_config, AiDifficulty, Platform};
use rand::rngs::SmallRng;
use rand::SeedableRng;

fn ai_choose_at_all_difficulties(
    state: &engine::types::game_state::GameState,
) -> Vec<(AiDifficulty, GameAction)> {
    [
        AiDifficulty::Easy,
        AiDifficulty::Medium,
        AiDifficulty::Hard,
        AiDifficulty::VeryHard,
    ]
    .into_iter()
    .map(|d| {
        let config = create_config(d, Platform::Native);
        let mut rng = SmallRng::seed_from_u64(42);
        let action =
            choose_action(state, P0, &config, &mut rng).expect("AI should return an action");
        (d, action)
    })
    .collect()
}

/// Resolve a real proliferate for P0 so the prompt has its resolution frame
/// (a hand-written `ProliferateChoice` has none: the engine refuses every
/// candidate and the AI falls back to "choose nothing", which proves nothing).
fn open_proliferate(state: &mut engine::types::game_state::GameState) {
    let ability = engine::types::ability::ResolvedAbility::new(
        engine::types::ability::Effect::Proliferate,
        vec![],
        ObjectId(999),
        P0,
    );
    engine::game::effects::proliferate::resolve(state, &ability, &mut Vec::new())
        .expect("proliferate resolves");
    assert!(matches!(
        state.waiting_for,
        WaitingFor::ProliferateChoice { .. }
    ));
}

fn proliferate_choices(
    state: &engine::types::game_state::GameState,
) -> Vec<(AiDifficulty, Vec<TargetRef>)> {
    let config = create_config(AiDifficulty::VeryHard, Platform::Native);
    assert!(
        !phase_ai::score_candidates(state, P0, &config).is_empty(),
        "the engine must offer proliferate candidates"
    );
    ai_choose_at_all_difficulties(state)
        .into_iter()
        .map(|(difficulty, action)| match action {
            GameAction::SelectTargets { targets } => (difficulty, targets),
            other => panic!("{difficulty:?} answered {other:?}"),
        })
        .collect()
}

/// The opponent's energy is the only thing carrying a counter: proliferating it
/// only helps the opponent. Every difficulty must choose nothing.
#[test]
fn proliferate_never_gives_the_opponent_energy() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PostCombatMain);
    let mut runner = scenario.build();
    let state = runner.state_mut();
    state.players[1].energy = 1;
    open_proliferate(state);
    for (difficulty, targets) in proliferate_choices(runner.state()) {
        assert!(
            targets.is_empty(),
            "{difficulty:?} proliferated the opponent's energy: {targets:?}"
        );
    }
}

/// Mine and theirs both carry +1/+1 counters and the opponent has energy:
/// grow only my creature.
#[test]
fn proliferate_grows_only_its_own_side() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PostCombatMain);
    let mine = scenario.add_vanilla(P0, 2, 2);
    let theirs = scenario.add_vanilla(P1, 2, 2);
    let mut runner = scenario.build();
    let state = runner.state_mut();
    for id in [mine, theirs] {
        state
            .objects
            .get_mut(&id)
            .unwrap()
            .counters
            .insert(engine::types::counter::CounterType::Plus1Plus1, 2);
    }
    state.players[1].energy = 2;
    open_proliferate(state);
    for (difficulty, targets) in proliferate_choices(runner.state()) {
        assert!(
            !targets.contains(&TargetRef::Object(theirs))
                && !targets.contains(&TargetRef::Player(P1)),
            "{difficulty:?} proliferated the opponent's side: {targets:?}"
        );
        // Easy samples at temperature 2 and may skip the proliferate; the
        // searching levels must take the helpful choice.
        if difficulty != AiDifficulty::Easy {
            assert_eq!(targets, vec![TargetRef::Object(mine)], "{difficulty:?}");
        }
    }
}
