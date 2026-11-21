//! Relatos da mesa de 21/11/2026 (rodada das 14h).
//!
//! 1. Omnath, Locus of All com Pillar of the Paruns / Obsidian Obelisk: "Spend
//!    this mana only to cast a multicolored spell" virava a restrição de TIPO
//!    "Multicolored", que nenhuma mágica tem — a mana nunca podia ser gasta e o
//!    Omnath (W U B/P R G) não era conjurável. CR 105.2c: multicolorido = duas ou
//!    mais cores; CR 106.6: restrição de gasto.
//! 2. Winota, Joiner of Forces: "Put the rest of the cards on the bottom of your
//!    library in a random order" vinha depois de "It gains indestructible until
//!    end of turn" e não alcançava o Dig — virava "escolha UMA carta para o
//!    fundo", uma segunda pergunta, e as outras olhadas ficavam no topo.
//!    CR 608.2c.
use engine::ai_support::legal_actions;
use engine::game::combat::AttackTarget;
use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::types::actions::GameAction;
use engine::types::game_state::WaitingFor;
use engine::types::identifiers::ObjectId;
use engine::types::mana::{ManaColor, ManaCost, ManaCostShard};
use engine::types::phase::Phase;

const PILLAR: &str =
    "{T}: Add one mana of any color. Spend this mana only to cast a multicolored spell.";

/// Forest + Pillar of the Paruns no campo e uma criatura de `shards` na mão.
fn castable_with_pillar(shards: Vec<ManaCostShard>, colors: Vec<ManaColor>) -> bool {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.add_basic_land(P0, ManaColor::Green);
    scenario.add_land_from_oracle(P0, "Pillar of the Paruns", PILLAR);
    let spell = scenario
        .add_creature_to_hand(P0, "Spell", 2, 2)
        .with_mana_cost(ManaCost::Cost { shards, generic: 0 })
        .with_color(colors)
        .id();
    let runner = scenario.build();
    legal_actions(runner.state())
        .iter()
        .any(|a| matches!(a, GameAction::CastSpell { object_id, .. } if *object_id == spell))
}

#[test]
fn multicolored_only_mana_pays_a_multicolored_spell() {
    assert!(
        castable_with_pillar(
            vec![ManaCostShard::Green, ManaCostShard::White],
            vec![ManaColor::Green, ManaColor::White],
        ),
        "a mana do Pillar paga mágica multicolorida (CR 105.2c + CR 106.6)"
    );
}

#[test]
fn multicolored_only_mana_does_not_pay_a_monocolored_spell() {
    assert!(
        !castable_with_pillar(
            vec![ManaCostShard::Green, ManaCostShard::Green],
            vec![ManaColor::Green],
        ),
        "mágica monocolorida não pode usar a mana do Pillar"
    );
}

const WINOTA: &str = "Whenever a non-Human creature you control attacks, look at the top six cards of your library. You may put a Human creature card from among them onto the battlefield tapped and attacking. It gains indestructible until end of turn. Put the rest of the cards on the bottom of your library in a random order.";

/// Winota + um Goblin atacando; biblioteca com 2 Humanos entre 8 cartas.
/// Devolve o runner depois de resolver o gatilho, as esperas vistas e os ids
/// (humano posto, humano que ficou, todas as cartas olhadas).
fn winota_attack() -> (GameRunner, Vec<&'static str>, ObjectId, Vec<ObjectId>) {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let winota = {
        let mut b = scenario.add_creature_from_oracle(P0, "Winota", 4, 4, WINOTA);
        b.with_subtypes(vec!["Human", "Warrior"]);
        b.id()
    };
    let goblin = {
        let mut b = scenario.add_creature(P0, "Goblin", 1, 1);
        b.with_subtypes(vec!["Goblin"]);
        b.id()
    };
    // Fundo → topo: as 6 de cima são as olhadas.
    let mut library = Vec::new();
    for i in 0..8 {
        let id = if i == 4 || i == 6 {
            let mut b = scenario.add_spell_to_library_top(P0, "Human", false);
            b.as_creature().with_subtypes(vec!["Human"]);
            b.id()
        } else {
            scenario.add_card_to_library_top(P0, "Filler")
        };
        library.push(id);
    }
    let looked: Vec<ObjectId> = library[2..].to_vec();
    let mut runner = scenario.build();
    runner.pass_both_players();
    runner
        .act(GameAction::DeclareAttackers {
            attacks: vec![
                (winota, AttackTarget::Player(P1)),
                (goblin, AttackTarget::Player(P1)),
            ],
            bands: vec![],
        })
        .expect("DeclareAttackers");
    let mut waits = Vec::new();
    let mut put = None;
    for _ in 0..20 {
        let w = runner.state().waiting_for.clone();
        waits.push(runner.waiting_for_kind());
        match w {
            WaitingFor::DigChoice {
                selectable_cards, ..
            } => {
                put = selectable_cards.first().copied();
                runner
                    .act(GameAction::SelectCards {
                        cards: put.into_iter().collect(),
                    })
                    .expect("DigChoice");
            }
            WaitingFor::EffectZoneChoice { cards, count, .. } => {
                runner
                    .act(GameAction::SelectCards {
                        cards: cards.into_iter().take(count).collect(),
                    })
                    .expect("EffectZoneChoice");
            }
            WaitingFor::Priority { .. } if !runner.state().stack.is_empty() => {
                runner.pass_both_players();
            }
            _ => break,
        }
    }
    (runner, waits, put.expect("um Humano para pôr"), looked)
}

#[test]
fn winota_asks_once_and_bottoms_every_other_looked_card() {
    let (runner, waits, put, looked) = winota_attack();
    assert_eq!(
        waits.iter().filter(|w| **w == "EffectZoneChoice").count(),
        0,
        "o resto vai ao fundo sem segunda pergunta: {waits:?}"
    );
    let st = runner.state();
    assert!(st.battlefield.contains(&put), "o Humano escolhido entra");
    let library = &st.players[0].library;
    // A biblioteca tem 7 cartas: as 2 não olhadas (fundo original) e as 5
    // olhadas que sobraram. As olhadas têm de estar TODAS abaixo das não olhadas.
    assert_eq!(library.len(), 7);
    // `library[0]` é o topo: as 2 não olhadas sobem ao topo e as 5 olhadas que
    // sobraram vão TODAS para baixo delas.
    assert!(
        library.iter().take(2).all(|id| !looked.contains(id)),
        "o topo são as 2 cartas não olhadas: {library:?} (olhadas {looked:?})"
    );
    assert!(
        st.objects[&put]
            .keywords
            .iter()
            .any(|k| matches!(k, engine::types::keywords::Keyword::Indestructible)),
        "o Humano posto ganha indestrutível"
    );
}
