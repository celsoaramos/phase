//! Relatos da mesa de 28/10/2026 (segunda leva).
//!
//! 1. New Horizons: "Enchanted land has '{T}: Add two mana of any one color.'" — virar o terreno
//!    dava as duas manas, mas o planejador de pagamento contava UMA: mágica de {1}{G} pagável só
//!    pela terra encantada não aparecia como conjurável (CR 106.1 + CR 605.3b).
//! 2. Cloudspire Skycycle: "distribute two +1/+1 counters among one or two other target Vehicles
//!    and/or creatures you control" — o "one or two" antes de "other target" se perdia e só dava
//!    para escolher UM alvo (CR 115.1d + CR 601.2d).
//! 3. Kalamax, the Stormsire: "Whenever you copy an instant spell, put a +1/+1 counter on
//!    Kalamax." — o gatilho tipado nem era lido (só "you copy a spell"); e a cópia feita no meio
//!    da resolução (Zada, Hedron Grinder) era coletada duas vezes (CR 707.10 + CR 603.2c).

use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::types::ability::TargetRef;
use engine::types::actions::GameAction;
use engine::types::counter::CounterType;
use engine::types::game_state::WaitingFor;
use engine::types::identifiers::ObjectId;
use engine::types::mana::ManaCost;
use engine::types::phase::Phase;
use engine::types::zones::Zone;

use engine::game::scenario_db::GameScenarioDbExt;

use super::support::shared_card_db;

fn build(scenario: GameScenario) -> GameRunner {
    let db = shared_card_db().expect("card db");
    let mut runner = scenario.build();
    engine::game::rehydrate_game_from_card_db(runner.state_mut(), db);
    runner
}

fn free_cost() -> ManaCost {
    ManaCost::Cost {
        shards: vec![],
        generic: 0,
    }
}

fn p1p1(r: &GameRunner, id: ObjectId) -> u32 {
    r.state().objects[&id]
        .counters
        .get(&CounterType::Plus1Plus1)
        .copied()
        .unwrap_or(0)
}

// ── 1. New Horizons ─────────────────────────────────────────────────────────

const NEW_HORIZONS: &str = "Enchant land\nWhen this Aura enters, put a +1/+1 counter on target creature you control.\nEnchanted land has \"{T}: Add two mana of any one color.\"";

#[test]
fn new_horizons_land_taps_for_two_mana() {
    let db = shared_card_db().expect("card db");
    let mut sc = GameScenario::new();
    sc.at_phase(Phase::PreCombatMain);
    let cave = sc.add_real_card(P0, "Cave of Temptation", Zone::Battlefield, db);
    let bears = sc.add_real_card(P0, "Grizzly Bears", Zone::Battlefield, db);
    let aura = sc
        .add_spell_to_hand(P0, "New Horizons", false)
        .as_enchantment()
        .with_subtypes(vec!["Aura"])
        .with_mana_cost(free_cost())
        .from_oracle_text_with_keywords(&["enchant"], NEW_HORIZONS)
        .id();
    let mut r = build(sc);
    let before = r.state().objects[&cave].abilities.len();
    r.cast(aura).target_object(cave).commit();
    for _ in 0..12 {
        match &r.state().waiting_for {
            WaitingFor::TriggerTargetSelection { .. } | WaitingFor::TargetSelection { .. } => {
                r.act(GameAction::ChooseTarget {
                    target: Some(TargetRef::Object(bears)),
                })
                .expect("target bears");
            }
            WaitingFor::Priority { .. } if !r.state().stack.is_empty() => {
                r.act(GameAction::PassPriority).expect("pass");
            }
            _ => break,
        }
    }
    assert!(
        r.state().objects[&aura].attached_to.is_some(),
        "a aura está anexada"
    );
    assert_eq!(p1p1(&r, bears), 1, "o gatilho de entrada pôs o marcador");
    let cave_obj = &r.state().objects[&cave];
    assert_eq!(
        cave_obj.abilities.len(),
        before + 1,
        "o terreno encantado ganhou a habilidade de mana da aura"
    );
}

/// O relato: só a Cave of Temptation (encantada) desvirada e uma mágica de {1}{G} na mão.
/// "Add two mana of any one color" paga o custo inteiro.
#[test]
fn new_horizons_land_alone_pays_a_two_mana_spell() {
    use engine::types::mana::ManaCostShard;
    let db = shared_card_db().expect("card db");
    let mut sc = GameScenario::new();
    sc.at_phase(Phase::PreCombatMain);
    let cave = sc.add_real_card(P0, "Cave of Temptation", Zone::Battlefield, db);
    let bears = sc.add_real_card(P0, "Grizzly Bears", Zone::Battlefield, db);
    let aura = sc
        .add_spell_to_hand(P0, "New Horizons", false)
        .as_enchantment()
        .with_subtypes(vec!["Aura"])
        .with_mana_cost(free_cost())
        .from_oracle_text_with_keywords(&["enchant"], NEW_HORIZONS)
        .id();
    let burst = sc
        .add_spell_to_hand_from_oracle(P0, "Biosynthic Burst", true, BURST)
        .with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::Green],
            generic: 1,
        })
        .id();
    let mut r = build(sc);
    r.cast(aura).target_object(cave).commit();
    for _ in 0..12 {
        match &r.state().waiting_for {
            WaitingFor::TriggerTargetSelection { .. } | WaitingFor::TargetSelection { .. } => {
                r.act(GameAction::ChooseTarget {
                    target: Some(TargetRef::Object(bears)),
                })
                .expect("target bears");
            }
            WaitingFor::Priority { .. } if !r.state().stack.is_empty() => {
                r.act(GameAction::PassPriority).expect("pass");
            }
            _ => break,
        }
    }
    assert!(!r.state().objects[&cave].tapped, "a Cave segue desvirada");
    let legal = engine::ai_support::legal_actions(r.state());
    let castable = legal
        .iter()
        .any(|a| matches!(a, GameAction::CastSpell { object_id, .. } if *object_id == burst));
    assert!(castable, "a Cave encantada paga {{1}}{{G}} sozinha");
    r.cast(burst).target_object(bears).commit();
    r.advance_until_stack_empty();
    assert!(r.state().objects[&cave].tapped, "a Cave pagou a mágica");
    assert_eq!(
        p1p1(&r, bears),
        2,
        "a Burst resolveu (1 da aura + 1 da Burst)"
    );
}

/// Controle do controle: duas Florestas pagam {1}{G} — a mágica do teste é conjurável.
#[test]
fn two_forests_pay_the_same_spell() {
    use engine::types::mana::{ManaColor, ManaCostShard};
    let db = shared_card_db().expect("card db");
    let mut sc = GameScenario::new();
    sc.at_phase(Phase::PreCombatMain);
    sc.add_basic_land(P0, ManaColor::Green);
    sc.add_basic_land(P0, ManaColor::Green);
    sc.add_real_card(P0, "Grizzly Bears", Zone::Battlefield, db);
    let burst = sc
        .add_spell_to_hand_from_oracle(P0, "Biosynthic Burst", true, BURST)
        .with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::Green],
            generic: 1,
        })
        .id();
    let r = build(sc);
    let legal = engine::ai_support::legal_actions(r.state());
    assert!(legal
        .iter()
        .any(|a| matches!(a, GameAction::CastSpell { object_id, .. } if *object_id == burst)));
}

/// Controle: uma Floresta com New Horizons paga a mesma mágica sozinha.
#[test]
fn new_horizons_on_a_forest_pays_a_two_mana_spell() {
    use engine::types::mana::{ManaColor, ManaCostShard};
    let db = shared_card_db().expect("card db");
    let mut sc = GameScenario::new();
    sc.at_phase(Phase::PreCombatMain);
    let forest = sc.add_basic_land(P0, ManaColor::Green);
    let bears = sc.add_real_card(P0, "Grizzly Bears", Zone::Battlefield, db);
    let aura = sc
        .add_spell_to_hand(P0, "New Horizons", false)
        .as_enchantment()
        .with_subtypes(vec!["Aura"])
        .with_mana_cost(free_cost())
        .from_oracle_text_with_keywords(&["enchant"], NEW_HORIZONS)
        .id();
    let burst = sc
        .add_spell_to_hand_from_oracle(P0, "Biosynthic Burst", true, BURST)
        .with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::Green],
            generic: 1,
        })
        .id();
    let mut r = build(sc);
    r.cast(aura).target_object(forest).commit();
    for _ in 0..12 {
        match &r.state().waiting_for {
            WaitingFor::TriggerTargetSelection { .. } | WaitingFor::TargetSelection { .. } => {
                r.act(GameAction::ChooseTarget {
                    target: Some(TargetRef::Object(bears)),
                })
                .expect("target bears");
            }
            WaitingFor::Priority { .. } if !r.state().stack.is_empty() => {
                r.act(GameAction::PassPriority).expect("pass");
            }
            _ => break,
        }
    }
    let legal = engine::ai_support::legal_actions(r.state());
    let castable = legal
        .iter()
        .any(|a| matches!(a, GameAction::CastSpell { object_id, .. } if *object_id == burst));
    assert!(castable, "a Floresta encantada paga {{1}}{{G}} sozinha");
}

// ── 2. Cloudspire Skycycle ──────────────────────────────────────────────────

const SKYCYCLE: &str = "Flying\nWhen this Vehicle enters, distribute two +1/+1 counters among one or two other target Vehicles and/or creatures you control.\nCrew 1";

/// Lança o Skycycle e responde à escolha de alvos com `picks` (depois, "pronto").
fn skycycle(picks: &[usize]) -> (GameRunner, ObjectId, ObjectId) {
    let db = shared_card_db().expect("card db");
    let mut sc = GameScenario::new();
    sc.at_phase(Phase::PreCombatMain);
    let bears = sc.add_real_card(P0, "Grizzly Bears", Zone::Battlefield, db);
    let angel = sc.add_real_card(P0, "Serra Angel", Zone::Battlefield, db);
    let cycle = sc
        .add_artifact_to_hand_from_oracle(P0, "Cloudspire Skycycle", SKYCYCLE)
        .with_subtypes(vec!["Vehicle"])
        .with_mana_cost(free_cost())
        .id();
    let mut r = build(sc);
    r.cast(cycle).commit();
    let ids = [bears, angel];
    let mut next = picks.iter();
    for _ in 0..20 {
        match r.state().waiting_for.clone() {
            WaitingFor::TriggerTargetSelection { .. } | WaitingFor::TargetSelection { .. } => {
                let target = next.next().map(|&k| TargetRef::Object(ids[k]));
                r.act(GameAction::ChooseTarget { target })
                    .expect("escolha de alvo");
            }
            WaitingFor::Priority { .. } if !r.state().stack.is_empty() => {
                r.act(GameAction::PassPriority).expect("pass");
            }
            WaitingFor::DistributeAmong { targets, total, .. } => {
                // CR 601.2d: cada alvo recebe pelo menos um; o resto vai no primeiro.
                let mut distribution: Vec<(TargetRef, u32)> =
                    targets.iter().map(|t| (t.clone(), 1)).collect();
                distribution[0].1 += total - targets.len() as u32;
                r.act(GameAction::DistributeAmong { distribution })
                    .expect("distribute");
            }
            WaitingFor::Priority { .. } => break,
            other => panic!("espera inesperada: {other:?}"),
        }
    }
    (r, bears, angel)
}

#[test]
fn cloudspire_skycycle_one_target_gets_both_counters() {
    let (r, bears, angel) = skycycle(&[0]);
    assert_eq!(p1p1(&r, bears), 2, "um alvo: os dois marcadores nele");
    assert_eq!(p1p1(&r, angel), 0);
}

#[test]
fn cloudspire_skycycle_two_targets_get_one_counter_each() {
    let (r, bears, angel) = skycycle(&[0, 1]);
    assert_eq!(p1p1(&r, bears), 1, "dois alvos: um marcador em cada");
    assert_eq!(p1p1(&r, angel), 1);
}

// ── 3. Kalamax + Zada ───────────────────────────────────────────────────────

const KALAMAX: &str = "Whenever you cast your first instant spell each turn, if Kalamax is tapped, copy that spell. You may choose new targets for the copy.\nWhenever you copy an instant spell, put a +1/+1 counter on Kalamax.";
const BURST: &str = "Put a +1/+1 counter on target creature you control. It gains reach, trample, and indestructible until end of turn. Untap it.";

#[test]
fn kalamax_gets_a_counter_for_each_copy_zada_makes() {
    let db = shared_card_db().expect("card db");
    let mut sc = GameScenario::new();
    sc.at_phase(Phase::PreCombatMain);
    let zada = sc.add_real_card(P0, "Zada, Hedron Grinder", Zone::Battlefield, db);
    let kalamax = sc
        .add_creature_from_oracle(P0, "Kalamax, the Stormsire", 4, 4, KALAMAX)
        .id();
    let bears = sc.add_real_card(P0, "Grizzly Bears", Zone::Battlefield, db);
    sc.add_real_card(P1, "Serra Angel", Zone::Battlefield, db);
    let burst = sc
        .add_spell_to_hand_from_oracle(P0, "Biosynthic Burst", true, BURST)
        .with_mana_cost(free_cost())
        .id();
    let mut r = build(sc);
    r.cast(burst).target_object(zada).commit();
    for _ in 0..60 {
        match &r.state().waiting_for {
            WaitingFor::Priority { .. } if !r.state().stack.is_empty() => {
                r.act(GameAction::PassPriority).expect("pass");
            }
            WaitingFor::Priority { .. } => break,
            other => panic!("espera inesperada: {other:?}"),
        }
    }
    // Zada copia para Kalamax e Bears (2 cópias): o Kalamax ganha 2 pelo gatilho de cópia
    // e 1 da cópia que mira nele.
    assert_eq!(p1p1(&r, zada), 1);
    assert_eq!(p1p1(&r, bears), 1);
    assert_eq!(
        p1p1(&r, kalamax),
        3,
        "2 pelas cópias + 1 da cópia que mira o Kalamax"
    );
}

// ── 4. Sapling Nursery: a ficha Treefolk tem alcance e bloqueia voador ─────

const SAPLING_NURSERY: &str = "Affinity for Forests (This spell costs {1} less to cast for each Forest you control.)\nLandfall — Whenever a land you control enters, create a 3/4 green Treefolk creature token with reach.\n{1}{G}, Exile this enchantment: Treefolk and Forests you control gain indestructible until end of turn.";

#[test]
fn sapling_nursery_treefolk_has_reach_and_blocks_a_flyer() {
    use engine::types::keywords::Keyword;
    let db = shared_card_db().expect("card db");
    let mut sc = GameScenario::new();
    sc.at_phase(Phase::PreCombatMain);
    sc.add_enchantment_from_oracle(P0, "Sapling Nursery", SAPLING_NURSERY);
    let land = sc.add_real_card(P0, "Forest", Zone::Hand, db);
    let pixie = sc
        .add_creature(P1, "Flying Witness", 1, 1)
        .with_keyword(Keyword::Flying)
        .id();
    let mut r = build(sc);
    let card_id = r.state().objects[&land].card_id;
    r.act(GameAction::PlayLand {
        object_id: land,
        card_id,
    })
    .expect("play land");
    r.advance_until_stack_empty();
    let tokens: Vec<ObjectId> = r
        .state()
        .battlefield
        .iter()
        .copied()
        .filter(|id| r.state().objects[id].name == "Treefolk")
        .collect();
    assert_eq!(tokens.len(), 1, "o landfall criou a ficha");
    let token = tokens[0];
    assert!(
        r.state().objects[&token].has_keyword(&Keyword::Reach),
        "a ficha 3/4 tem alcance"
    );
    assert!(
        engine::game::combat::can_block_pair(r.state(), token, pixie),
        "alcance bloqueia voador"
    );
}

// ── 5. Tenth District Hero: "Collect evidence" exige cemitério ─────────────

#[test]
fn tenth_district_hero_collect_evidence_needs_graveyard_value() {
    let db = shared_card_db().expect("card db");
    let build_with = |graveyard: Option<&str>| {
        let mut sc = GameScenario::new();
        sc.at_phase(Phase::PreCombatMain);
        let hero = sc.add_real_card(P0, "Tenth District Hero", Zone::Battlefield, db);
        sc.add_real_card(P0, "Plains", Zone::Battlefield, db);
        sc.add_real_card(P0, "Plains", Zone::Battlefield, db);
        if let Some(name) = graveyard {
            sc.add_real_card(P0, name, Zone::Graveyard, db);
        }
        (build(sc), hero)
    };
    let offered = |r: &GameRunner, hero: ObjectId| {
        engine::ai_support::legal_actions(r.state()).iter().any(
            |a| matches!(a, GameAction::ActivateAbility { source_id, .. } if *source_id == hero),
        )
    };
    let (r, hero) = build_with(None);
    assert!(
        !offered(&r, hero),
        "cemitério vazio: não há o que exilar para reunir provas 2"
    );
    // Grizzly Bears tem valor de mana 2: paga "Collect evidence 2".
    let (r, hero) = build_with(Some("Grizzly Bears"));
    assert!(
        offered(&r, hero),
        "com valor 2 no cemitério, a habilidade é oferecida"
    );
}

// ── 6. Bloqueio da mesa do relato 94dcb4c4 ─────────────────────────────────

/// A mesa do relato: seis atacantes do robô (um voador) contra Treefolk 3/4 com alcance,
/// Llanowar Elves, Mightform Harmonizer e um Chocobo 12/13. Todo bloqueador pode bloquear
/// todo atacante de chão, e o voador só por quem tem alcance.
#[test]
fn reported_board_block_targets() {
    use engine::game::combat::AttackTarget;
    use engine::types::keywords::Keyword;
    let db = shared_card_db().expect("card db");
    let mut sc = GameScenario::new();
    sc.at_phase(Phase::PreCombatMain);
    let pixie = sc
        .add_creature(P1, "Nurturing Pixie", 2, 2)
        .with_keyword(Keyword::Flying)
        .id();
    let gearhulk = sc
        .add_creature(P1, "Brightglass Gearhulk", 8, 8)
        .with_keyword(Keyword::FirstStrike)
        .with_keyword(Keyword::Trample)
        .id();
    let spider = sc
        .add_creature(P1, "Spider Manifestation", 4, 4)
        .with_keyword(Keyword::Reach)
        .id();
    let fungus = sc.add_creature(P1, "Insidious Fungus", 5, 6).id();
    let cub = sc.add_real_card(P1, "Badgermole Cub", Zone::Battlefield, db);
    let pollinator = sc.add_real_card(P1, "Gene Pollinator", Zone::Battlefield, db);
    let treefolk: Vec<ObjectId> = (0..2)
        .map(|_| {
            sc.add_creature(P0, "Treefolk", 3, 4)
                .with_keyword(Keyword::Reach)
                .id()
        })
        .collect();
    let elves = sc.add_real_card(P0, "Llanowar Elves", Zone::Battlefield, db);
    let harmonizer = sc.add_real_card(P0, "Mightform Harmonizer", Zone::Battlefield, db);
    let chocobo = sc.add_creature(P0, "Sazh's Chocobo", 12, 13).id();
    let mut r = build(sc);
    r.state_mut().active_player = P1;
    r.advance_to_combat();
    let attackers = [pixie, gearhulk, spider, fungus, cub, pollinator];
    let attacks: Vec<_> = attackers
        .iter()
        .map(|&a| (a, AttackTarget::Player(P0)))
        .collect();
    r.declare_attackers(&attacks).expect("declare attackers");
    for _ in 0..8 {
        if matches!(r.state().waiting_for, WaitingFor::DeclareBlockers { .. }) {
            break;
        }
        r.act(GameAction::PassPriority).expect("pass");
    }
    let WaitingFor::DeclareBlockers {
        valid_blocker_ids,
        valid_block_targets,
        ..
    } = r.state().waiting_for.clone()
    else {
        panic!("esperava DeclareBlockers, veio {:?}", r.state().waiting_for);
    };
    let ground = [gearhulk, spider, fungus, cub, pollinator];
    for &b in treefolk.iter().chain([elves, harmonizer, chocobo].iter()) {
        assert!(valid_blocker_ids.contains(&b), "{b:?} pode bloquear");
        let targets = valid_block_targets.get(&b).cloned().unwrap_or_default();
        for a in ground {
            assert!(
                targets.contains(&a),
                "{b:?} deveria poder bloquear {a:?}: {targets:?}"
            );
        }
    }
    for &t in &treefolk {
        assert!(
            valid_block_targets[&t].contains(&pixie),
            "alcance bloqueia o voador"
        );
    }
}
