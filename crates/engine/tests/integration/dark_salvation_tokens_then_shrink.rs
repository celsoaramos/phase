//! Dark Salvation (relato de campo 27/10/2026): "Target player creates X 2/2 black Zombie
//! creature tokens, then up to one target creature gets -1/-1 until end of turn for each Zombie
//! that player controls."
//!
//! CR 608.2c + CR 111.2 + CR 601.2c: o parser guardava só UMA das orações (o `Token` sumia quando
//! a 2ª tinha "for each"; o `Pump` sumia quando não tinha). A mágica resolvia sem efeito.
//! Casos discriminantes: o jogador alvo é quem cria e quem tem os Zumbis contados ("that
//! player"), não o controlador da mágica; X = 0 não cria nada nem encolhe nada.

use engine::game::scenario::{GameScenario, P0, P1};
use engine::types::game_state::GameState;
use engine::types::identifiers::ObjectId;
use engine::types::mana::{ManaCost, ManaCostShard, ManaType, ManaUnit};
use engine::types::phase::Phase;
use engine::types::player::PlayerId;

const DARK_SALVATION: &str = "Target player creates X 2/2 black Zombie creature tokens, then up to one target creature gets -1/-1 until end of turn for each Zombie that player controls.";

fn mana(black: usize, generic: usize) -> Vec<ManaUnit> {
    let mut pool: Vec<ManaUnit> = (0..black)
        .map(|_| ManaUnit::new(ManaType::Black, ObjectId(0), false, vec![]))
        .collect();
    pool.extend(
        (0..generic).map(|_| ManaUnit::new(ManaType::Colorless, ObjectId(0), false, vec![])),
    );
    pool
}

fn zombies(state: &GameState, player: PlayerId) -> usize {
    state
        .battlefield
        .iter()
        .filter_map(|id| state.objects.get(id))
        .filter(|o| {
            o.controller == player
                && o.is_token
                && o.card_types.subtypes.iter().any(|s| s == "Zombie")
                && o.power == Some(2)
                && o.toughness == Some(2)
        })
        .count()
}

fn scenario() -> (GameScenario, ObjectId, ObjectId) {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    // {X}{X}{B} com X = 2 → 5 de mana.
    scenario.with_mana_pool(P0, mana(1, 4));
    let spell = scenario
        .add_spell_to_hand_from_oracle(P0, "Dark Salvation", false, DARK_SALVATION)
        .with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::X, ManaCostShard::X, ManaCostShard::Black],
            generic: 0,
        })
        .id();
    let victim = scenario.add_creature(P1, "Big Bear", 4, 4).id();
    (scenario, spell, victim)
}

/// O jogador alvo é o OPONENTE: ele cria os Zumbis e os Zumbis DELE é que são contados.
#[test]
fn opponent_creates_the_zombies_and_the_shrink_counts_them() {
    let (scenario, spell, victim) = scenario();
    let mut runner = scenario.build();
    let outcome = runner
        .cast(spell)
        .x(2)
        .target_player(P1)
        .target_object(victim)
        .resolve();
    assert_eq!(zombies(outcome.state(), P1), 2, "o alvo cria X fichas");
    assert_eq!(
        zombies(outcome.state(), P0),
        0,
        "o controlador não ganha ficha"
    );
    outcome.assert_power_toughness(victim, 2, 2);
}

/// O jogador alvo é o PRÓPRIO controlador: as fichas são dele e a criatura do oponente encolhe.
#[test]
fn caster_targets_self_and_shrinks_the_opponents_creature() {
    let (scenario, spell, victim) = scenario();
    let mut runner = scenario.build();
    let outcome = runner
        .cast(spell)
        .x(2)
        .target_player(P0)
        .target_object(victim)
        .resolve();
    assert_eq!(zombies(outcome.state(), P0), 2);
    assert_eq!(zombies(outcome.state(), P1), 0);
    outcome.assert_power_toughness(victim, 2, 2);
}

/// "Up to one": sem criatura alvo, as fichas saem do mesmo jeito.
#[test]
fn creature_target_is_optional() {
    let (scenario, spell, victim) = scenario();
    let mut runner = scenario.build();
    let outcome = runner.cast(spell).x(2).target_player(P0).resolve();
    assert_eq!(zombies(outcome.state(), P0), 2);
    outcome.assert_power_toughness(victim, 4, 4);
}

/// X = 0: nenhuma ficha e nenhum -1/-1.
#[test]
fn x_zero_creates_nothing_and_shrinks_nothing() {
    let (scenario, spell, victim) = scenario();
    let mut runner = scenario.build();
    let outcome = runner
        .cast(spell)
        .x(0)
        .target_player(P0)
        .target_object(victim)
        .resolve();
    assert_eq!(zombies(outcome.state(), P0), 0);
    outcome.assert_power_toughness(victim, 4, 4);
}
