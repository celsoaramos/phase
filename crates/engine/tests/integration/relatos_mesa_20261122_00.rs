//! Relatos da mesa de 22/11/2026 (rodada das 00h).
//!
//! "Travou": desafio de Commander com Ouroboroid + Branching Evolution + The
//! Ozolith — as criaturas do jogador chegaram a ~2,3 bilhões de marcadores
//! +1/+1, com poder no teto do `i32`. Dois atacantes assim somam mais dano do
//! que a vida do defensor cabe num `i32` abaixo de zero: a perda de vida era
//! "irrepresentável", o motor recusava o PassPriority inteiro que levava ao
//! dano de combate e devolvia a mesma prioridade — a mesa passava para sempre.
//! A perda que já é letal agora para no piso do `i32`: o jogador fica com vida
//! ≤ 0 e perde pela ação de estado (CR 704.5a), que é o que a regra pede.
use engine::game::scenario::{GameScenario, P0, P1};
use engine::types::game_state::WaitingFor;
use engine::types::phase::Phase;

use crate::rules::run_combat;

#[test]
fn two_attackers_at_the_i32_power_cap_kill_instead_of_freezing_combat() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let a = scenario
        .add_creature(P0, "Ouroboroid", i32::MAX, i32::MAX)
        .id();
    let b = scenario
        .add_creature(P0, "Spine Sower", i32::MAX, i32::MAX)
        .id();
    let mut runner = scenario.build();

    run_combat(&mut runner, vec![a, b], vec![]);
    for _ in 0..20 {
        if matches!(runner.state().waiting_for, WaitingFor::GameOver { .. }) {
            break;
        }
        let _ = runner.act(engine::types::actions::GameAction::PassPriority);
    }

    let st = runner.state();
    assert!(
        matches!(st.waiting_for, WaitingFor::GameOver { .. }),
        "o dano de combate tem de matar o defensor, não travar: {:?} (fase {:?}, vida {})",
        st.waiting_for,
        st.phase,
        runner.life(P1)
    );
    assert!(runner.life(P1) <= 0);
}

#[test]
fn a_single_attacker_at_the_cap_still_deals_exact_damage() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let a = scenario
        .add_creature(P0, "Ouroboroid", i32::MAX, i32::MAX)
        .id();
    let mut runner = scenario.build();
    let before = runner.life(P1);

    run_combat(&mut runner, vec![a], vec![]);

    assert_eq!(
        i64::from(runner.life(P1)),
        i64::from(before) - i64::from(i32::MAX)
    );
}
