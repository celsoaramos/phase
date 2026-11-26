//! MagicFinder flags of 25/11/2026 (`tactical_gate/mf_flags_1126.rs`), all from
//! the robot's mirror of a Sliver Commander deck (robot = seat 1).
//!
//! - f5d3e488: Rite of Replication copied the robot's commander, The Wandering
//!   Minstrel, and the legend rule took the token.
//! - 25b8bcba: Shapesharer became a copy of the robot's Sliver Hivelord and was
//!   put into the graveyard by the legend rule.
//! - b8efa67b: Budoka Gardener tapped to put a land from a hand without lands.

use engine::game::engine::apply_as_current_for_simulation;
use engine::game::scenario::P1;
use engine::types::ability::TargetRef;
use engine::types::actions::GameAction;
use engine::types::card_type::CoreType;
use engine::types::game_state::{GameState, WaitingFor};
use engine::types::identifiers::ObjectId;
use engine::types::player::PlayerId;
use phase_ai::config::{create_config, AiDifficulty, Platform};
use phase_ai::saved_state::load_saved_game_state;
use phase_ai::score_candidates;
use std::io::Read;

fn scored(state: &GameState, player: PlayerId) -> Vec<GameAction> {
    let config = create_config(AiDifficulty::VeryHard, Platform::Native);
    score_candidates(state, player, &config)
        .into_iter()
        .map(|(action, _)| action)
        .collect()
}

fn engine_actions(state: &GameState) -> Vec<GameAction> {
    engine::ai_support::build_decision_context(state)
        .candidates
        .iter()
        .map(|c| c.action.clone())
        .collect()
}

fn load(bytes: &[u8]) -> GameState {
    let mut raw = String::new();
    flate2::read::GzDecoder::new(bytes)
        .read_to_string(&mut raw)
        .expect("fixture inflates");
    load_saved_game_state(&raw).expect("position restores")
}

fn named(state: &GameState, name: &str) -> Vec<ObjectId> {
    state
        .battlefield
        .iter()
        .copied()
        .filter(|id| state.objects[id].name == name)
        .collect()
}

fn hand_card(state: &GameState, player: PlayerId, name: &str) -> ObjectId {
    state
        .players
        .iter()
        .find(|p| p.id == player)
        .expect("player")
        .hand
        .iter()
        .copied()
        .find(|id| state.objects[id].name == name)
        .expect("card in hand")
}

fn chooses_object(action: &GameAction, ids: &[ObjectId]) -> bool {
    match action {
        GameAction::ChooseTarget {
            target: Some(TargetRef::Object(id)),
        } => ids.contains(id),
        GameAction::SelectTargets { targets } => targets
            .iter()
            .any(|t| matches!(t, TargetRef::Object(id) if ids.contains(id))),
        _ => false,
    }
}

/// Plays `start`, then the AI's own choices, until the engine offers one of
/// `legends` as a target; returns the AI's scored candidates there.
fn scored_at_target_choice(
    mut state: GameState,
    start: GameAction,
    legends: &[ObjectId],
) -> (GameState, Vec<GameAction>) {
    apply_as_current_for_simulation(&mut state, start).expect("the flagged action is legal");
    for _ in 0..12 {
        let actions = scored(&state, P1);
        if matches!(
            state.waiting_for,
            WaitingFor::TargetSelection { .. } | WaitingFor::MultiTargetSelection { .. }
        ) && engine_actions(&state)
            .iter()
            .any(|a| chooses_object(a, legends))
        {
            return (state, actions);
        }
        let next = actions.into_iter().next().expect("the AI has an action");
        apply_as_current_for_simulation(&mut state, next).expect("the AI's choice is legal");
    }
    panic!("no target choice reached");
}

#[test]
fn flag_f5d3e488_rite_of_replication_does_not_copy_a_minstrel() {
    let state = load(include_bytes!(
        "fixtures/scenarios/mf-flag-f5d3e488-rite-of-replication.json.gz"
    ));
    let minstrels = named(&state, "The Wandering Minstrel");
    assert_eq!(minstrels.len(), 2, "o comandante dos dois lados do espelho");
    let rite = hand_card(&state, P1, "Rite of Replication");
    let cast = engine_actions(&state)
        .into_iter()
        .find(|a| matches!(a, GameAction::CastSpell { object_id, .. } if *object_id == rite))
        .expect("o motor oferece a Rite");
    let (state, actions) = scored_at_target_choice(state, cast, &minstrels);
    let engine_targets = engine_actions(&state);
    assert!(
        engine_targets.iter().any(|a| chooses_object(a, &minstrels)),
        "o motor deixa mirar o Minstrel"
    );
    assert!(
        !actions.iter().any(|a| chooses_object(a, &minstrels)),
        "copiar um Minstrel com o Minstrel do robô no campo perde a cópia (regra da lenda)"
    );
    assert!(
        actions.iter().any(|a| matches!(
            a,
            GameAction::ChooseTarget { .. } | GameAction::SelectTargets { .. }
        )),
        "copiar um Sliver segue aberto"
    );
}

#[test]
fn flag_25b8bcba_shapesharer_does_not_copy_the_hivelord() {
    let state = load(include_bytes!(
        "fixtures/scenarios/mf-flag-25b8bcba-shapesharer.json.gz"
    ));
    let shapesharer = named(&state, "Shapesharer")[0];
    let mut legends = named(&state, "Sliver Hivelord");
    legends.extend(named(&state, "The Wandering Minstrel"));
    let activate = engine_actions(&state)
        .into_iter()
        .find(|a| matches!(a, GameAction::ActivateAbility { source_id, .. } if *source_id == shapesharer))
        .expect("o motor oferece a Shapesharer");
    let (state, actions) = scored_at_target_choice(state, activate, &legends);
    assert!(
        engine_actions(&state)
            .iter()
            .any(|a| chooses_object(a, &legends)),
        "o motor deixa copiar a Hivelord"
    );
    assert!(
        !actions.iter().any(|a| chooses_object(a, &legends)),
        "a Shapesharer virando cópia de uma lenda do robô vai para o cemitério"
    );
}

fn budoka() -> (GameState, ObjectId) {
    let state = load(include_bytes!(
        "fixtures/scenarios/mf-flag-b8efa67b-budoka-gardener.json.gz"
    ));
    let budoka = named(&state, "Budoka Gardener")[0];
    (state, budoka)
}

fn offers_activation(actions: &[GameAction], source: ObjectId) -> bool {
    actions
        .iter()
        .any(|a| matches!(a, GameAction::ActivateAbility { source_id, .. } if *source_id == source))
}

#[test]
fn flag_b8efa67b_budoka_gardener_needs_a_land_in_hand() {
    let (state, budoka) = budoka();
    assert!(offers_activation(&engine_actions(&state), budoka));
    assert!(
        !offers_activation(&scored(&state, P1), budoka),
        "sem terreno na mão, virar o Budoka não faz nada"
    );
}

/// The same position with Phyrexian Arena turned into a land card: now there is
/// something to put onto the battlefield.
#[test]
fn budoka_gardener_with_a_land_in_hand_stays_open() {
    let (mut state, budoka) = budoka();
    let arena = hand_card(&state, P1, "Phyrexian Arena");
    let object = state.objects.get_mut(&arena).unwrap();
    object.card_types.core_types = vec![CoreType::Land];
    object.base_card_types.core_types = vec![CoreType::Land];
    assert!(
        offers_activation(&scored(&state, P1), budoka),
        "com terreno na mão, o gate não recusa"
    );
}
