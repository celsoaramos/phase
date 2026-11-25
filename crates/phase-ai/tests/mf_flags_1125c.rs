//! Sacrifice-for-mana the payment does not need (`tactical_gate/mf_flags_1125c.rs`,
//! MagicFinder flag e8fac237): paying Tyvar's Stand ({X}{G}, X = 2), the robot
//! kept sacrificing creatures to Ashnod's Altar with four colorless floating and
//! Arcane Signet untapped — the attacking Meteor Golem was next.

use engine::game::scenario::P1;
use engine::types::actions::GameAction;
use engine::types::game_state::{GameState, WaitingFor};
use engine::types::identifiers::ObjectId;
use engine::types::card_type::CoreType;
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

fn offers_activation(actions: &[GameAction], source: ObjectId) -> bool {
    actions
        .iter()
        .any(|a| matches!(a, GameAction::ActivateAbility { source_id, .. } if *source_id == source))
}

fn engine_offers(state: &GameState, source: ObjectId) -> bool {
    let actions: Vec<GameAction> = engine::ai_support::build_decision_context(state)
        .candidates
        .iter()
        .map(|c| c.action.clone())
        .collect();
    offers_activation(&actions, source)
}

/// The captured position of flag e8fac237: the robot (seat 1) mid-payment.
#[test]
fn flag_e8fac237_robot_stops_sacrificing_when_the_cost_is_covered() {
    let (state, altar) = position();
    assert!(matches!(state.waiting_for, WaitingFor::ManaPayment { player, .. } if player == P1));
    assert!(engine_offers(&state, altar), "o motor oferece o Altar");
    let actions = scored(&state, P1);
    assert!(
        !offers_activation(&actions, altar),
        "a reserva e o Arcane Signet pagam o resto: sacrificar mais uma criatura não serve"
    );
    assert!(actions.contains(&GameAction::PassPriority), "pagar com o que tem segue aberto");
}

fn position() -> (GameState, ObjectId) {
    let mut raw = String::new();
    flate2::read::GzDecoder::new(
        &include_bytes!("fixtures/scenarios/mf-flag-e8fac237-ashnods-altar.json.gz")[..],
    )
    .read_to_string(&mut raw)
    .expect("fixture inflates");
    let state = load_saved_game_state(&raw).expect("position restores");
    let altar = state
        .battlefield
        .iter()
        .copied()
        .find(|id| state.objects[id].name == "Ashnod's Altar")
        .expect("Ashnod's Altar on the battlefield");
    (state, altar)
}

/// The same position with the floating mana gone: Arcane Signet alone cannot pay
/// {2}{G}, so the Altar is the way to finish the payment.
#[test]
fn robot_may_sacrifice_when_the_payment_needs_it() {
    let (mut state, altar) = position();
    state.players[1].mana_pool.clear();
    assert!(engine_offers(&state, altar));
    assert!(
        offers_activation(&scored(&state, P1), altar),
        "sem a reserva, o Altar é o que paga: o gate não recusa"
    );
}

/// The same position with the robot's creatures turned into Eggs: Atla Palani's
/// "whenever an Egg you control dies" makes the sacrifice the point.
#[test]
fn robot_may_sacrifice_into_a_death_payoff() {
    let (mut state, altar) = position();
    let creatures: Vec<ObjectId> = state
        .battlefield
        .iter()
        .copied()
        .filter(|id| {
            let o = &state.objects[id];
            o.controller == P1 && o.card_types.core_types.contains(&CoreType::Creature)
        })
        .collect();
    for id in creatures {
        let o = state.objects.get_mut(&id).unwrap();
        o.card_types.subtypes.push("Egg".to_string());
        o.base_card_types.subtypes.push("Egg".to_string());
    }
    assert!(engine_offers(&state, altar));
    assert!(
        offers_activation(&scored(&state, P1), altar),
        "sacrificar um Ovo dispara a Atla Palani: o gate não recusa"
    );
}
