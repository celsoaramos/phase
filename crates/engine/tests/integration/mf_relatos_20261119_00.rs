//! Relatos da mesa de 18/11/2026 (rodada de 19/11 00h UTC).
//!
//! - Najeela, the Blade-Blossom: "{W}{U}{B}{R}{G}: Untap all attacking
//!   creatures. They gain trample, lifelink, and haste until end of turn."
//! - Tajuru Preserver × Soul Shatter: "Spells and abilities your opponents
//!   control can't cause you to sacrifice permanents."
//! - Winternight Stories: "discard two cards unless you discard a creature card".

use engine::game::combat::AttackTarget;
use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::types::actions::GameAction;
use engine::types::game_state::{CastPaymentMode, WaitingFor};
use engine::types::identifiers::ObjectId;
use engine::types::keywords::Keyword;
use engine::types::mana::{ManaCost, ManaCostShard, ManaType, ManaUnit};
use engine::types::phase::Phase;
use engine::types::player::PlayerId;
use engine::types::zones::Zone;

const NAJEELA: &str = "Whenever a Warrior attacks, you may have its controller create a 1/1 white Warrior creature token that's tapped and attacking.\n{W}{U}{B}{R}{G}: Untap all attacking creatures. They gain trample, lifelink, and haste until end of turn. After this phase, there is an additional combat phase. Activate only during combat.";

fn add_mana(runner: &mut GameRunner, player: PlayerId, tys: &[ManaType]) {
    let p = runner
        .state_mut()
        .players
        .iter_mut()
        .find(|p| p.id == player)
        .unwrap();
    for ty in tys {
        p.mana_pool
            .add(ManaUnit::new(*ty, ObjectId(0), false, vec![]));
    }
}

fn drain(runner: &mut GameRunner) {
    for _ in 0..40 {
        match runner.state().waiting_for.clone() {
            WaitingFor::OptionalEffectChoice { .. } => {
                runner
                    .act(GameAction::DecideOptionalEffect { accept: true })
                    .unwrap();
            }
            WaitingFor::Priority { .. } if !runner.state().stack.is_empty() => {
                runner.act(GameAction::PassPriority).unwrap();
            }
            _ => break,
        }
    }
}

fn has_kw(runner: &GameRunner, id: ObjectId, kw: &Keyword) -> bool {
    runner.state().objects[&id].has_keyword(kw)
}

/// Najeela: a criatura atacante que NÃO é Guerreiro desvira e ganha
/// atropelar, vínculo com a vida e ímpeto, como o Guerreiro.
#[test]
fn najeela_grants_keywords_to_every_attacking_creature() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let najeela = {
        let mut b =
            scenario.add_creature_from_oracle(P0, "Najeela, the Blade-Blossom", 3, 2, NAJEELA);
        b.with_subtypes(vec!["Human", "Warrior"]);
        b.id()
    };
    let angel = scenario.add_creature(P0, "Serra Angel", 4, 4).id();
    let mut runner = scenario.build();
    runner.advance_to_combat();
    runner
        .declare_attackers(&[
            (najeela, AttackTarget::Player(P1)),
            (angel, AttackTarget::Player(P1)),
        ])
        .unwrap();
    drain(&mut runner);
    assert!(
        runner.state().objects[&angel].tapped,
        "pré: o anjo atacou virado"
    );
    use ManaType::*;
    add_mana(&mut runner, P0, &[White, Blue, Black, Red, Green]);
    runner
        .act(GameAction::ActivateAbility {
            source_id: najeela,
            ability_index: 0,
            payment_mode: CastPaymentMode::Auto,
        })
        .unwrap();
    drain(&mut runner);
    let tokens: Vec<ObjectId> = runner
        .state()
        .battlefield
        .iter()
        .copied()
        .filter(|id| runner.state().objects[id].is_token)
        .collect();
    for id in [najeela, angel].into_iter().chain(tokens) {
        let name = runner.state().objects[&id].name.clone();
        assert!(!runner.state().objects[&id].tapped, "{name} desvira");
        for kw in [Keyword::Trample, Keyword::Lifelink, Keyword::Haste] {
            assert!(has_kw(&runner, id, &kw), "{name} ganha {kw:?}");
        }
    }
}

const SACRIFICE_PROTECTION: &str =
    "Spells and abilities your opponents control can't cause you to sacrifice permanents.";
const SOUL_SHATTER: &str = "Each opponent sacrifices a creature or planeswalker with the greatest mana value among creatures and planeswalkers they control.";

/// Soul Shatter do oponente não faz o protegido pelo Tajuru Preserver sacrificar.
#[test]
fn soul_shatter_is_muzzled_by_tajuru_preserver() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let tajuru = scenario
        .add_creature_from_oracle(P0, "Tajuru Preserver", 3, 2, SACRIFICE_PROTECTION)
        .with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::Green],
            generic: 1,
        })
        .id();
    let big = scenario
        .add_creature(P0, "Ashaya, Soul of the Wild", 4, 4)
        .with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::Green, ManaCostShard::Green],
            generic: 3,
        })
        .id();
    let shatter = scenario
        .add_spell_to_hand_from_oracle(P1, "Soul Shatter", true, SOUL_SHATTER)
        .with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::Black],
            generic: 2,
        })
        .id();
    let mut runner = scenario.build();
    add_mana(&mut runner, P1, &[ManaType::Black; 3]);
    runner.state_mut().priority_player = P1;
    runner.state_mut().waiting_for = WaitingFor::Priority { player: P1 };
    let outcome = runner.cast(shatter).effect_zone(&[big]).resolve();
    outcome.assert_zone(&[tajuru, big], Zone::Battlefield);
}

const WINTERNIGHT: &str =
    "Draw three cards. Then discard two cards unless you discard a creature card.";

/// Winternight Stories: descartar UMA criatura cumpre o "a menos que".
#[test]
fn winternight_stories_accepts_one_creature_discard() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let spell = scenario
        .add_spell_to_hand_from_oracle(P0, "Winternight Stories", false, WINTERNIGHT)
        .with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::Blue],
            generic: 2,
        })
        .id();
    let creature = scenario.add_creature_to_hand(P0, "Sea Scourge", 3, 3).id();
    scenario.add_card_to_hand(P0, "Mountain");
    scenario.with_library_top(P0, &["Island", "Island", "Island", "Island"]);
    let mut runner = scenario.build();
    add_mana(&mut runner, P0, &[ManaType::Blue; 3]);
    runner
        .act(GameAction::CastSpell {
            object_id: spell,
            card_id: runner.state().objects[&spell].card_id,
            targets: vec![],
            payment_mode: CastPaymentMode::Auto,
        })
        .ok();
    for _ in 0..10 {
        if matches!(runner.state().waiting_for, WaitingFor::DiscardChoice { .. }) {
            break;
        }
        if runner.act(GameAction::PassPriority).is_err() {
            break;
        }
    }
    assert!(
        matches!(runner.state().waiting_for, WaitingFor::DiscardChoice { .. }),
        "{:?}",
        runner.state().waiting_for
    );
    // A mesa só deixa confirmar uma carta quando o motor LISTA essa seleção.
    let legal = engine::ai_support::legal_actions(runner.state());
    assert!(
        legal.contains(&GameAction::SelectCards {
            cards: vec![creature]
        }),
        "a criatura sozinha é listada: {legal:?}"
    );
    runner
        .act(GameAction::SelectCards {
            cards: vec![creature],
        })
        .unwrap();
    assert_eq!(runner.state().objects[&creature].zone, Zone::Graveyard);
    assert_eq!(
        runner.state().players[0].graveyard.len(),
        2,
        "a criatura e a mágica"
    );
}

/// Controle: sem o Tajuru, o Soul Shatter sacrifica a de maior valor de mana.
#[test]
fn soul_shatter_without_protection_sacrifices_greatest() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let small = scenario
        .add_creature(P0, "Grizzly Bears", 2, 2)
        .with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::Green],
            generic: 1,
        })
        .id();
    let big = scenario
        .add_creature(P0, "Ashaya, Soul of the Wild", 4, 4)
        .with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::Green, ManaCostShard::Green],
            generic: 3,
        })
        .id();
    let shatter = scenario
        .add_spell_to_hand_from_oracle(P1, "Soul Shatter", true, SOUL_SHATTER)
        .with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::Black],
            generic: 2,
        })
        .id();
    let mut runner = scenario.build();
    add_mana(&mut runner, P1, &[ManaType::Black; 3]);
    runner.state_mut().priority_player = P1;
    runner.state_mut().waiting_for = WaitingFor::Priority { player: P1 };
    let outcome = runner.cast(shatter).effect_zone(&[big]).resolve();
    outcome.assert_zone(&[big], Zone::Graveyard);
    outcome.assert_zone(&[small], Zone::Battlefield);
}
