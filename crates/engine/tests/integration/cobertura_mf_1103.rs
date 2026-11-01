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

use engine::game::commander::record_commander_cast;
use engine::game::effects::attach;
use engine::game::scenario::{GameScenario, P0, P1};
use engine::parser::oracle::parse_oracle_text;
use engine::types::ability::Effect;
use engine::types::actions::GameAction;
use engine::types::game_state::WaitingFor;
use engine::types::identifiers::ObjectId;
use engine::types::mana::{ManaCost, ManaType, ManaUnit};
use engine::types::phase::Phase;
use engine::types::player::PlayerId;
use engine::types::zones::Zone;

const DEVOUT_HARPIST: &str = "{T}: Destroy target Aura attached to a creature.";

#[test]
fn devout_harpist_targets_only_auras_attached_to_creatures() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let harpist = scenario
        .add_creature(P0, "Devout Harpist", 1, 1)
        .from_oracle_text(DEVOUT_HARPIST)
        .id();
    let ogre = scenario.add_creature(P1, "Enemy Ogre", 3, 3).id();
    let field = scenario
        .add_land_from_oracle(P1, "Field", "{T}: Add {G}.")
        .id();
    let on_ogre = aura(&mut scenario, P1, "Aura On Ogre", "Enchant creature");
    let on_field = aura(&mut scenario, P1, "Aura On Field", "Enchant land");
    let mut runner = scenario.build();
    attach::attach_to(runner.state_mut(), on_ogre, ogre);
    attach::attach_to(runner.state_mut(), on_field, field);

    assert!(
        runner
            .activate(harpist, 0)
            .target_object(on_field)
            .try_resolve()
            .is_err(),
        "an Aura attached to a land is not a legal target"
    );
    assert_eq!(runner.state().objects[&on_field].zone, Zone::Battlefield);

    runner.activate(harpist, 0).target_object(on_ogre).resolve();
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
