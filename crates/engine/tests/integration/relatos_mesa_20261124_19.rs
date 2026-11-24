//! Relatos da mesa de 24/11/2026 (rodada de 24/11 19h UTC).
//!
//! Nivmagus Elemental — "Exile an instant or sorcery spell you control: Put two
//! +1/+1 counters on this creature." O custo exila uma MÁGICA (objeto na pilha,
//! CR 112.1), não uma carta da mão. O parser deixava o custo sem zona e o motor
//! inferia a MÃO: a habilidade era ativada com a pilha vazia, sem exilar nada.
use engine::game::scenario::{GameScenario, P0};
use engine::types::actions::GameAction;
use engine::types::counter::CounterType;
use engine::types::game_state::{CastPaymentMode, WaitingFor};
use engine::types::mana::ManaCost;
use engine::types::phase::Phase;
use engine::types::zones::Zone;

const NIVMAGUS: &str =
    "Exile an instant or sorcery spell you control: Put two +1/+1 counters on this creature.";

#[test]
fn nivmagus_cannot_activate_without_a_spell_on_the_stack() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let niv = scenario
        .add_creature_from_oracle(P0, "Nivmagus Elemental", 1, 2, NIVMAGUS)
        .id();
    let sorcery = scenario
        .add_spell_to_hand_from_oracle(P0, "Arc Trail", false, "Draw a card.")
        .with_mana_cost(ManaCost::generic(5))
        .id();
    let mut runner = scenario.build();
    let _ = runner.act(GameAction::ActivateAbility {
        source_id: niv,
        ability_index: 0,
        payment_mode: CastPaymentMode::Auto,
    });
    // Resolve o que tiver ido à pilha.
    for _ in 0..4 {
        if matches!(runner.state().waiting_for, WaitingFor::Priority { .. })
            && runner.state().stack.is_empty()
        {
            break;
        }
        let _ = runner.act(GameAction::PassPriority);
    }
    assert_eq!(
        runner.state().objects[&sorcery].zone,
        Zone::Hand,
        "feitiço na MÃO não paga o custo"
    );
    assert_eq!(
        runner.counters(niv, CounterType::Plus1Plus1),
        0,
        "sem mágica na pilha não há o que exilar"
    );
}

#[test]
fn nivmagus_exiles_own_instant_spell_from_the_stack() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let niv = scenario
        .add_creature_from_oracle(P0, "Nivmagus Elemental", 1, 2, NIVMAGUS)
        .id();
    let instant = scenario
        .add_spell_to_hand_from_oracle(P0, "Opt", true, "Draw a card.")
        .with_mana_cost(ManaCost::generic(0))
        .id();
    let mut runner = scenario.build();
    let card_id = runner.state().objects[&instant].card_id;
    runner
        .act(GameAction::CastSpell {
            object_id: instant,
            card_id,
            targets: vec![],
            payment_mode: CastPaymentMode::Auto,
        })
        .expect("conjura");
    assert_eq!(runner.state().objects[&instant].zone, Zone::Stack);
    runner
        .act(GameAction::ActivateAbility {
            source_id: niv,
            ability_index: 0,
            payment_mode: CastPaymentMode::Auto,
        })
        .expect("ativa com a mágica na pilha");
    assert_eq!(
        runner.state().objects[&instant].zone,
        Zone::Exile,
        "a mágica é exilada da pilha para pagar o custo; espera = {:?}",
        runner.state().waiting_for
    );
    for _ in 0..4 {
        if runner.state().stack.is_empty() {
            break;
        }
        let _ = runner.act(GameAction::PassPriority);
    }
    assert_eq!(runner.counters(niv, CounterType::Plus1Plus1), 2);
}
