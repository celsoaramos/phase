//! Relatos da mesa de 21/11/2026.
//!
//! Engine Rat ("Deathtouch") com Tainted Strike ("gains infect") bloqueado por
//! Leatherback Baloth (4/5): o dano de infectar vira marcadores -1/-1 e não marca
//! dano, e a ação de estado do toque mortífero exigia dano MARCADO — o Baloth ficava
//! vivo como 2/3.
//!
//! CR 702.2b: qualquer dano de fonte com toque mortífero é letal.
//! CR 702.90: dano de infectar a uma criatura vira marcadores -1/-1.
//! CR 704.5h: criatura com resistência > 0 que recebeu dano de fonte com toque
//! mortífero desde a última checagem é destruída.
use engine::game::scenario::{GameScenario, P0, P1};
use engine::types::actions::GameAction;
use engine::types::keywords::Keyword;
use engine::types::phase::Phase;

use engine::game::combat::AttackTarget;

fn attack_and_block(
    attacker_keywords: &[Keyword],
) -> (
    bool,
    engine::types::identifiers::ObjectId,
    engine::game::scenario::GameRunner,
) {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let attacker = {
        let mut b = scenario.add_creature(P0, "Rat", 2, 1);
        for kw in attacker_keywords {
            b.with_keyword(kw.clone());
        }
        b.id()
    };
    let blocker = scenario.add_creature(P1, "Baloth", 4, 5).id();

    let mut runner = scenario.build();
    runner.pass_both_players();
    runner
        .act(GameAction::DeclareAttackers {
            attacks: vec![(attacker, AttackTarget::Player(P1))],
            bands: vec![],
        })
        .expect("DeclareAttackers");
    runner.pass_both_players();
    runner
        .act(GameAction::DeclareBlockers {
            assignments: vec![(blocker, attacker)],
        })
        .expect("DeclareBlockers");
    runner.pass_both_players();
    let alive = runner.state().battlefield.contains(&blocker);
    (alive, blocker, runner)
}

#[test]
fn deathtouch_infect_damage_destroys_blocker() {
    let (alive, _, _) = attack_and_block(&[Keyword::Deathtouch, Keyword::Infect]);
    assert!(
        !alive,
        "dano de infectar de fonte com toque mortífero é letal (CR 702.2b + CR 704.5h)"
    );
}

#[test]
fn deathtouch_wither_damage_destroys_blocker() {
    let (alive, _, _) = attack_and_block(&[Keyword::Deathtouch, Keyword::Wither]);
    assert!(!alive, "murchar com toque mortífero também é letal");
}

#[test]
fn infect_without_deathtouch_only_shrinks_blocker() {
    let (alive, blocker, runner) = attack_and_block(&[Keyword::Infect]);
    assert!(alive, "sem toque mortífero, 2 marcadores num 4/5 não matam");
    assert_eq!(runner.state().objects[&blocker].toughness, Some(3));
}

/// Hidetsugu's Second Rite (relato de 07/11): "If target player has exactly 10
/// life, Hidetsugu's Second Rite deals 10 damage to that player." Com vida
/// diferente de 10 não pode haver dano.
const SECOND_RITE: &str =
    "If target player has exactly 10 life, Hidetsugu's Second Rite deals 10 damage to that player.";

fn second_rite_life_after(life: i32) -> i32 {
    second_rite_lives(life, P0).0
}

/// Vida de (P0, P1) depois de conjurar o Rite com `target` a `life` de vida.
fn second_rite_lives(life: i32, target: engine::types::player::PlayerId) -> (i32, i32) {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.with_life(target, life);
    let rite = scenario
        .add_spell_to_hand_from_oracle(P0, "Hidetsugu's Second Rite", true, SECOND_RITE)
        .id();
    let mut runner = scenario.build();
    let outcome = runner.cast(rite).target_player(target).resolve();
    let st = outcome.state();
    (st.players[0].life, st.players[1].life)
}

#[test]
fn second_rite_does_nothing_without_exactly_ten_life() {
    assert_eq!(second_rite_life_after(36), 36);
}

#[test]
fn second_rite_deals_ten_at_exactly_ten_life() {
    assert_eq!(second_rite_life_after(10), 0);
}

#[test]
fn second_rite_hits_the_targeted_opponent() {
    assert_eq!(second_rite_lives(10, P1), (20, 0));
}
