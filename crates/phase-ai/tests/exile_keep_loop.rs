//! Jogada ruim de 15/11/2026 (MagicFinder, motor v0.125.0, desafiante Thranduil):
//! o robô conjurou **Tainted Pact** — "Exile the top card of your library. You
//! may put that card into your hand unless it has the same name as another card
//! exiled this way. Repeat this process until you put a card into your hand or
//! you exile two cards with the same name, whichever comes first." — e recusou
//! TODA carta: 90 cartas exiladas, grimório zerado.
//!
//! A projeção da busca dá a mesma nota a aceitar e recusar (a carta exilada não
//! pesa na avaliação) e o empate cai em `accept: false`, o primeiro candidato.

use engine::game::scenario::{GameRunner, GameScenario, P0};
use engine::types::actions::GameAction;
use engine::types::game_state::{CastPaymentMode, WaitingFor};
use engine::types::identifiers::ObjectId;
use engine::types::mana::{ManaCost, ManaCostShard, ManaType, ManaUnit};
use engine::types::phase::Phase;
use phase_ai::choose_action;
use phase_ai::config::{create_config, AiDifficulty, Platform};
use rand::rngs::SmallRng;
use rand::SeedableRng;

const TAINTED_PACT: &str = "Exile the top card of your library. You may put that card into your \
hand unless it has the same name as another card exiled this way. Repeat this process until you \
put a card into your hand or you exile two cards with the same name, whichever comes first.";

/// Tainted Pact resolving for P0, parked on the first "you may put that card
/// into your hand". `top_is_land` picks what the first exiled card is; `lands`
/// is how many lands P0 controls.
fn pact_choice(top_is_land: bool, lands: usize, library: usize) -> GameRunner {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.with_mana_pool(
        P0,
        (0..2)
            .map(|_| ManaUnit::new(ManaType::Black, ObjectId(0), false, vec![]))
            .collect(),
    );
    for _ in 0..lands {
        scenario.add_basic_land(P0, engine::types::mana::ManaColor::Black);
    }
    for i in 0..library {
        let name = format!("Filler Spell {i}");
        scenario.add_spell_to_library_top(P0, &name, false);
    }
    if top_is_land {
        scenario.add_land_to_library_top(P0, "Bojuka Bog");
    } else {
        scenario.add_spell_to_library_top(P0, "Top Spell", true);
    }
    let pact = {
        let mut card =
            scenario.add_spell_to_hand_from_oracle(P0, "Tainted Pact", true, TAINTED_PACT);
        card.with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::Black],
            generic: 1,
        });
        card.id()
    };
    let mut runner = scenario.build();
    let card_id = runner.state().objects[&pact].card_id;
    runner
        .act(GameAction::CastSpell {
            object_id: pact,
            card_id,
            targets: vec![],
            payment_mode: CastPaymentMode::Auto,
        })
        .expect("cast Tainted Pact");
    for _ in 0..12 {
        match runner.state().waiting_for {
            WaitingFor::OptionalEffectChoice { .. } => return runner,
            _ => {
                runner.act(GameAction::PassPriority).expect("pass / pay");
            }
        }
    }
    panic!("Tainted Pact never asked: {:?}", runner.state().waiting_for);
}

fn decisions(runner: &GameRunner) -> Vec<GameAction> {
    let config = create_config(AiDifficulty::VeryHard, Platform::Native);
    (0..6)
        .map(|seed| {
            let mut rng = SmallRng::seed_from_u64(seed);
            choose_action(runner.state(), P0, &config, &mut rng).expect("AI acts")
        })
        .collect()
}

#[test]
fn keeps_the_first_spell_instead_of_exiling_the_library() {
    let runner = pact_choice(false, 4, 30);
    for action in decisions(&runner) {
        assert_eq!(
            action,
            GameAction::DecideOptionalEffect { accept: true },
            "declining a spell keeps exiling the library"
        );
    }
}

#[test]
fn keeps_a_land_when_short_on_lands() {
    let runner = pact_choice(true, 1, 30);
    for action in decisions(&runner) {
        assert_eq!(action, GameAction::DecideOptionalEffect { accept: true });
    }
}

#[test]
fn keeps_whatever_comes_when_the_library_is_almost_gone() {
    let runner = pact_choice(true, 6, 6);
    for action in decisions(&runner) {
        assert_eq!(action, GameAction::DecideOptionalEffect { accept: true });
    }
}
