//! Quarto lote de cobertura (medição de 30/10/2026, build `v0.119.0-mf`).
//! Cada teste passa pelo pipeline real de conjurar/resolver.
//!
//! - Devout Harpist (CR 115.1 + CR 303.4b + CR 701.3a): "Destroy target Aura
//!   attached to a creature" — a forma sem "that's" do hospedeiro indefinido
//!   (a forma "that's attached to a creature" é a do Sage's Reverie, coberta
//!   em `cobertura_mf_1031`). Aura presa a terreno não é alvo legal.
//! - Study Hall (CR 106.6 + CR 603.2 + CR 903.8): "When you spend this mana to
//!   cast your commander, scry X, where X is the number of times it's been
//!   cast from the command zone this game" — X é a contagem DESSE comandante
//!   (incluindo esta conjuração), não a soma com o parceiro (o lote 1031 lia a
//!   soma).
//! - Garruk, Cursed Huntsman (CR 201.5c + CR 205.3j + CR 606.3): o token Lobo
//!   "put a loyalty counter on each Garruk you control" — "Garruk" é o nome
//!   curto da carta mas também subtipo de planeswalker; cada Garruk que você
//!   controla ganha o marcador, e um planeswalker de outro subtipo não.
//! - Nine-Lives Familiar (CR 614.1c + CR 601.2): "enters with eight revival
//!   counters on it if you cast it" — conjurada de qualquer zona entra com 8;
//!   posta no campo sem ser conjurada entra sem nenhum.

use engine::game::commander::record_commander_cast;
use engine::game::effects::attach;
use engine::game::scenario::{GameScenario, P0, P1};
use engine::parser::oracle::parse_oracle_text;
use engine::types::ability::{Effect, TargetRef};
use engine::types::actions::GameAction;
use engine::types::counter::{parse_counter_type, CounterType};
use engine::types::game_state::{CastPaymentMode, WaitingFor};
use engine::types::identifiers::ObjectId;
use engine::types::mana::{ManaCost, ManaType, ManaUnit};
use engine::types::phase::Phase;
use engine::types::player::PlayerId;
use engine::types::zones::Zone;

const DEVOUT_HARPIST: &str = "{T}: Destroy target Aura attached to a creature.";

fn aura(scenario: &mut GameScenario, owner: PlayerId, name: &str, oracle: &str) -> ObjectId {
    scenario
        .add_enchantment_from_oracle(owner, name, oracle)
        .with_subtypes(vec!["Aura"])
        .id()
}

#[test]
fn devout_harpist_targets_only_auras_attached_to_creatures() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let harpist = scenario
        .add_creature(P0, "Devout Harpist", 1, 1)
        .from_oracle_text(DEVOUT_HARPIST)
        .id();
    let ogre = scenario.add_creature(P1, "Enemy Ogre", 3, 3).id();
    let bear = scenario.add_creature(P0, "Ally Bear", 2, 2).id();
    let field = scenario
        .add_land_from_oracle(P1, "Field", "{T}: Add {G}.")
        .id();
    let on_ogre = aura(&mut scenario, P1, "Aura On Ogre", "Enchant creature");
    let on_bear = aura(&mut scenario, P0, "Aura On Bear", "Enchant creature");
    let on_field = aura(&mut scenario, P1, "Aura On Field", "Enchant land");
    let mut runner = scenario.build();
    attach::attach_to(runner.state_mut(), on_ogre, ogre);
    attach::attach_to(runner.state_mut(), on_bear, bear);
    attach::attach_to(runner.state_mut(), on_field, field);

    runner
        .act(GameAction::ActivateAbility {
            source_id: harpist,
            ability_index: 0,
            payment_mode: CastPaymentMode::Auto,
        })
        .expect("activate Devout Harpist");
    let WaitingFor::TargetSelection { target_slots, .. } = runner.state().waiting_for.clone()
    else {
        panic!(
            "expected a target prompt, got {:?}",
            runner.state().waiting_for
        );
    };
    let mut legal: Vec<ObjectId> = target_slots
        .iter()
        .flat_map(|slot| slot.legal_targets.iter())
        .filter_map(|target| match target {
            TargetRef::Object(id) => Some(*id),
            TargetRef::Player(_) => None,
        })
        .collect();
    legal.sort();
    let mut expected = vec![on_ogre, on_bear];
    expected.sort();
    assert_eq!(
        legal, expected,
        "only the Auras attached to creatures are legal targets, not the one on a land"
    );

    runner
        .act(GameAction::SelectTargets {
            targets: vec![TargetRef::Object(on_ogre)],
        })
        .expect("target the Aura on the Ogre");
    for _ in 0..8 {
        if runner.state().stack.is_empty() {
            break;
        }
        runner.act(GameAction::PassPriority).expect("pass priority");
    }
    assert_eq!(runner.state().objects[&on_ogre].zone, Zone::Graveyard);
    assert_eq!(runner.state().objects[&on_field].zone, Zone::Battlefield);
}

const STUDY_HALL: &str = "{T}: Add {C}.\n{1}, {T}: Add one mana of any color. When you spend this \
    mana to cast your commander, scry X, where X is the number of times it's been cast from the \
    command zone this game.";

#[test]
fn study_hall_scries_by_that_commanders_own_cast_count() {
    let parsed = parse_oracle_text(STUDY_HALL, "Study Hall", &[], &["Land".to_string()], &[]);
    let grants = parsed
        .abilities
        .iter()
        .find_map(|ability| match &*ability.effect {
            Effect::Mana { grants, .. } if !grants.is_empty() => Some(grants.clone()),
            _ => None,
        })
        .expect("Study Hall's second mana ability carries the spend trigger");

    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let hall = scenario
        .add_land_from_oracle(P0, "Study Hall", STUDY_HALL)
        .id();
    let commander = scenario
        .add_creature(P0, "Hall Commander", 2, 2)
        .with_mana_cost(ManaCost::generic(1))
        .id();
    scenario.with_commander(commander);
    let partner = scenario.add_creature(P0, "Hall Partner", 2, 2).id();
    scenario.with_commander(partner);
    for i in 0..8 {
        scenario.add_land_to_library_top(P0, &format!("Library Land {i}"));
    }
    let mut hall_mana = ManaUnit::new(ManaType::Blue, hall, false, vec![]);
    hall_mana.grants = grants;
    scenario.with_mana_pool(
        P0,
        vec![
            hall_mana,
            ManaUnit::new(ManaType::Colorless, ObjectId(0), false, vec![]),
            ManaUnit::new(ManaType::Colorless, ObjectId(0), false, vec![]),
        ],
    );
    let mut runner = scenario.build();
    runner.state_mut().format_config.command_zone = true;
    // One earlier cast of this commander (tax {2}), three of the partner.
    record_commander_cast(runner.state_mut(), commander);
    for _ in 0..3 {
        record_commander_cast(runner.state_mut(), partner);
    }

    runner.cast(commander).commit();
    let mut scried = Vec::new();
    for _ in 0..32 {
        match runner.state().waiting_for.clone() {
            WaitingFor::Priority { .. } if runner.state().stack.is_empty() => break,
            WaitingFor::Priority { .. } => {
                runner.act(GameAction::PassPriority).expect("pass priority");
            }
            WaitingFor::ScryChoice { cards, .. } => {
                scried.push(cards.len());
                runner
                    .act(GameAction::SelectCards { cards })
                    .expect("keep the scried cards on top");
            }
            other => panic!("unexpected prompt while resolving: {other:?}"),
        }
    }
    assert_eq!(
        scried,
        vec![2],
        "scry 2: the earlier cast plus this one; the partner's three casts don't count"
    );
    assert_eq!(runner.state().objects[&commander].zone, Zone::Battlefield);
}

fn drain_stack(runner: &mut engine::game::scenario::GameRunner) {
    for _ in 0..16 {
        if runner.state().stack.is_empty() {
            return;
        }
        runner.act(GameAction::PassPriority).expect("pass priority");
    }
    panic!("stack did not empty: {:?}", runner.state().waiting_for);
}

fn counter_count(
    runner: &engine::game::scenario::GameRunner,
    id: ObjectId,
    kind: &CounterType,
) -> u32 {
    runner.state().objects[&id]
        .counters
        .get(kind)
        .copied()
        .unwrap_or(0)
}

const GARRUK: &str = "[0]: Create two 2/2 black and green Wolf creature tokens with \"When this \
    token dies, put a loyalty counter on each Garruk you control.\"\n[\u{2212}3]: Destroy target \
    creature. Draw a card.\n[\u{2212}6]: You get an emblem with \"Creatures you control get +3/+3 \
    and have trample.\"";

#[test]
fn garruk_wolf_death_adds_loyalty_to_each_garruk_you_control() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let garruk = scenario
        .add_planeswalker_from_oracle(P0, "Garruk, Cursed Huntsman", "Garruk", 5, GARRUK)
        .id();
    let other_garruk = scenario
        .add_planeswalker_from_oracle(P0, "Garruk Stand-in", "Garruk", 3, "")
        .id();
    let jace = scenario
        .add_planeswalker_from_oracle(P0, "Jace Stand-in", "Jace", 3, "")
        .id();
    let enemy_garruk = scenario
        .add_planeswalker_from_oracle(P1, "Enemy Garruk", "Garruk", 3, "")
        .id();
    let bolt = scenario.add_bolt_to_hand(P0);
    let mut runner = scenario.build();

    runner
        .act(GameAction::ActivateAbility {
            source_id: garruk,
            ability_index: 0,
            payment_mode: CastPaymentMode::Auto,
        })
        .expect("activate Garruk's [0]");
    drain_stack(&mut runner);
    let wolves: Vec<ObjectId> = runner
        .state()
        .battlefield
        .iter()
        .copied()
        .filter(|id| runner.state().objects[id].name.contains("Wolf"))
        .collect();
    assert_eq!(wolves.len(), 2, "the [0] makes two Wolf tokens");

    let loyalty = CounterType::Loyalty;
    assert_eq!(counter_count(&runner, garruk, &loyalty), 5);
    runner.cast(bolt).target_object(wolves[0]).resolve();
    drain_stack(&mut runner);
    assert!(
        !runner.state().battlefield.contains(&wolves[0]),
        "the bolted Wolf died"
    );
    assert_eq!(counter_count(&runner, garruk, &loyalty), 6, "Garruk itself");
    assert_eq!(
        counter_count(&runner, other_garruk, &loyalty),
        4,
        "every Garruk you control, not only the source"
    );
    assert_eq!(counter_count(&runner, jace, &loyalty), 3, "not a Garruk");
    assert_eq!(
        counter_count(&runner, enemy_garruk, &loyalty),
        3,
        "a Garruk you don't control"
    );
}

const NINE_LIVES_FAMILIAR: &str = "This creature enters with eight revival counters on it if you \
    cast it.\nWhen this creature dies, if it had a revival counter on it, return it to the \
    battlefield with one fewer revival counter on it at the beginning of the next end step.";

#[test]
fn nine_lives_familiar_gets_revival_counters_only_when_cast() {
    let revival = parse_counter_type("revival");

    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let cast_one = scenario
        .add_creature_to_hand_from_oracle(P0, "Nine-Lives Familiar", 1, 1, NINE_LIVES_FAMILIAR)
        .id();
    let mut runner = scenario.build();
    runner.cast(cast_one).resolve();
    drain_stack(&mut runner);
    assert_eq!(runner.state().objects[&cast_one].zone, Zone::Battlefield);
    assert_eq!(
        counter_count(&runner, cast_one, &revival),
        8,
        "cast from hand: enters with eight revival counters"
    );

    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let reanimated = scenario
        .add_creature_to_graveyard(P0, "Nine-Lives Familiar", 1, 1)
        .from_oracle_text(NINE_LIVES_FAMILIAR)
        .id();
    let reanimate = scenario
        .add_spell_to_hand_from_oracle(
            P0,
            "Reanimate Stand-in",
            false,
            "Return target creature card from your graveyard to the battlefield.",
        )
        .id();
    let mut runner = scenario.build();
    runner.cast(reanimate).target_object(reanimated).resolve();
    drain_stack(&mut runner);
    assert_eq!(runner.state().objects[&reanimated].zone, Zone::Battlefield);
    assert_eq!(
        counter_count(&runner, reanimated, &revival),
        0,
        "put onto the battlefield without being cast: no revival counters"
    );
}
