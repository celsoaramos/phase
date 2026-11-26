//! MagicFinder flags of 26/11/2026 (`tactical_gate/mf_flags_1126b.rs`).
//!
//! - e2cd4180: Heroic Intervention cast in the robot's own combat damage step,
//!   with nothing of the opponent's on the stack.
//! - dcaaede3: Seam Rip cast against an opponent with one land and no other
//!   permanent (robot = seat 1).

use engine::game::combat::{AttackerInfo, CombatState};
use engine::game::scenario::{GameScenario, P0, P1};
use engine::types::actions::GameAction;
use engine::types::game_state::{GameState, WaitingFor};
use engine::types::identifiers::ObjectId;
use engine::types::phase::Phase;
use engine::types::player::PlayerId;
use phase_ai::config::{create_config, AiDifficulty, Platform};
use phase_ai::saved_state::load_saved_game_state;
use phase_ai::score_candidates;
use std::io::Read;

const HEROIC_INTERVENTION: &str =
    "Permanents you control gain hexproof and indestructible until end of turn.";
const SEAM_RIP: &str = "When this artifact enters, exile target nonland permanent an opponent controls with mana value 2 or less until this artifact leaves the battlefield.";

fn scored(state: &GameState, player: PlayerId) -> Vec<GameAction> {
    let config = create_config(AiDifficulty::VeryHard, Platform::Native);
    score_candidates(state, player, &config)
        .into_iter()
        .map(|(action, _)| action)
        .collect()
}

fn offers_cast(actions: &[GameAction], card: ObjectId) -> bool {
    actions
        .iter()
        .any(|a| matches!(a, GameAction::CastSpell { object_id, .. } if *object_id == card))
}

fn engine_offers_cast(state: &GameState, card: ObjectId) -> bool {
    let actions: Vec<GameAction> = engine::ai_support::build_decision_context(state)
        .candidates
        .iter()
        .map(|c| c.action.clone())
        .collect();
    offers_cast(&actions, card)
}

fn at(phase: Phase, scenario: GameScenario) -> GameState {
    let runner = scenario.build();
    let mut state = runner.state().clone();
    state.phase = phase;
    state.active_player = P0;
    state.priority_player = P0;
    state.waiting_for = WaitingFor::Priority { player: P0 };
    state
}

fn intervention_board(phase: Phase) -> (GameState, ObjectId, ObjectId) {
    let mut scenario = GameScenario::new();
    let bear = scenario.add_creature(P0, "Grizzly Bears", 2, 2).id();
    let spell = scenario
        .add_spell_to_hand_from_oracle(P0, "Heroic Intervention", true, HEROIC_INTERVENTION)
        .id();
    scenario.add_creature(P1, "Hill Giant", 3, 3);
    (at(phase, scenario), spell, bear)
}

#[test]
fn flag_e2cd4180_protection_with_nothing_to_protect_against_is_not_cast() {
    for phase in [Phase::End, Phase::PostCombatMain, Phase::CombatDamage] {
        let (state, spell, _) = intervention_board(phase);
        assert!(engine_offers_cast(&state, spell));
        assert!(
            !offers_cast(&scored(&state, P0), spell),
            "{phase:?}: nada na pilha e nenhum dano por vir, a proteção não para nada"
        );
    }
}

#[test]
fn protection_stays_open_while_combat_damage_is_ahead() {
    let (mut state, spell, bear) = intervention_board(Phase::DeclareBlockers);
    state.combat = Some(CombatState {
        attackers: vec![AttackerInfo::attacking_player(bear, P1)],
        ..CombatState::default()
    });
    assert!(engine_offers_cast(&state, spell));
    assert!(
        offers_cast(&scored(&state, P0), spell),
        "antes do dano de combate a proteção ainda pode salvar uma criatura"
    );
}

fn seam_rip_board(opponent_two_drop: bool) -> (GameState, ObjectId) {
    let mut scenario = GameScenario::new();
    let rip = scenario
        .add_artifact_to_hand_from_oracle(P0, "Seam Rip", SEAM_RIP)
        .id();
    if opponent_two_drop {
        scenario.add_artifact_from_oracle(P1, "Mind Stone", "{T}: Add {C}.");
    }
    (at(Phase::PreCombatMain, scenario), rip)
}

#[test]
fn enters_removal_with_no_legal_target_is_not_cast() {
    let (state, rip) = seam_rip_board(false);
    assert!(engine_offers_cast(&state, rip));
    assert!(
        !offers_cast(&scored(&state, P0), rip),
        "o único efeito é o 'quando entra', e ele não tem alvo"
    );
}

#[test]
fn enters_removal_with_a_target_stays_open() {
    let (mut state, rip) = seam_rip_board(true);
    // Mind Stone's printed mana value is 2.
    let stone = state
        .battlefield
        .iter()
        .copied()
        .find(|id| state.objects[id].name == "Mind Stone")
        .expect("Mind Stone on the battlefield");
    state.objects.get_mut(&stone).unwrap().mana_cost = engine::types::mana::ManaCost::Cost {
        shards: vec![],
        generic: 2,
    };
    assert!(engine_offers_cast(&state, rip));
    assert!(offers_cast(&scored(&state, P0), rip));
}

#[test]
fn flag_dcaaede3_seam_rip_against_an_empty_board_is_not_cast() {
    let mut raw = String::new();
    flate2::read::GzDecoder::new(
        &include_bytes!("fixtures/scenarios/mf-flag-dcaaede3-seam-rip.json.gz")[..],
    )
    .read_to_string(&mut raw)
    .expect("fixture inflates");
    let state = load_saved_game_state(&raw).expect("position restores");
    let rip = state
        .players
        .iter()
        .find(|p| p.id == P1)
        .expect("robot")
        .hand
        .iter()
        .copied()
        .find(|id| state.objects[id].name == "Seam Rip")
        .expect("Seam Rip in the robot's hand");
    assert!(
        engine_offers_cast(&state, rip),
        "o motor oferece a Seam Rip"
    );
    assert!(
        !offers_cast(&scored(&state, P1), rip),
        "o oponente não tem permanente não-terreno: a Seam Rip não faria nada"
    );
}
