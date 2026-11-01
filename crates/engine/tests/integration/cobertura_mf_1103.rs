//! Quarto lote de cobertura (medição de 30/10/2026, build `v0.119.0-mf`).
//! Cada teste passa pelo pipeline real de conjurar/resolver.
//!
//! - Sage's Reverie (CR 303.4b + CR 701.3a + CR 613.4c): "draw a card for each
//!   Aura you control that's attached to a creature" e "Enchanted creature
//!   gets +1/+1 for each Aura you control that's attached to a creature" — a
//!   população conta só Auras SUAS presas a uma CRIATURA (de qualquer
//!   controlador); Aura do oponente e Aura presa a terreno ficam de fora.
//! - Study Hall (CR 106.6 + CR 603.2 + CR 903.8): "When you spend this mana to
//!   cast your commander, scry X, where X is the number of times it's been
//!   cast from the command zone this game" — X é a contagem DESSE comandante
//!   (incluindo esta conjuração), não a soma com o parceiro.

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

const SAGES_REVERIE: &str = "Enchant creature\nWhen this Aura enters, draw a card for each Aura \
    you control that's attached to a creature.\nEnchanted creature gets +1/+1 for each Aura you \
    control that's attached to a creature.";

fn aura(scenario: &mut GameScenario, owner: PlayerId, name: &str, oracle: &str) -> ObjectId {
    scenario
        .add_enchantment_from_oracle(owner, name, oracle)
        .with_subtypes(vec!["Aura"])
        .id()
}

#[test]
fn sages_reverie_counts_your_auras_attached_to_creatures() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let bear = scenario.add_creature(P0, "Ally Bear", 2, 2).id();
    let ogre = scenario.add_creature(P1, "Enemy Ogre", 3, 3).id();
    let field = scenario
        .add_land_from_oracle(P0, "Field", "{T}: Add {G}.")
        .id();
    // Counted: mine on my creature, mine on the opponent's creature.
    let mine_on_bear = aura(&mut scenario, P0, "Mine On Bear", "Enchant creature");
    let mine_on_ogre = aura(&mut scenario, P0, "Mine On Ogre", "Enchant creature");
    // Not counted: the opponent's Aura, and my Aura attached to a land.
    let theirs_on_bear = aura(&mut scenario, P1, "Theirs On Bear", "Enchant creature");
    let mine_on_land = aura(&mut scenario, P0, "Mine On Land", "Enchant land");
    for _ in 0..4 {
        scenario.add_land_to_library_top(P0, "Library Land");
    }
    let reverie = scenario
        .add_spell_to_hand(P0, "Sage's Reverie", false)
        .as_enchantment()
        .with_subtypes(vec!["Aura"])
        .with_mana_cost(ManaCost::zero())
        .from_oracle_text_with_keywords(&["enchant"], SAGES_REVERIE)
        .id();
    let mut runner = scenario.build();
    attach::attach_to(runner.state_mut(), mine_on_bear, bear);
    attach::attach_to(runner.state_mut(), mine_on_ogre, ogre);
    attach::attach_to(runner.state_mut(), theirs_on_bear, bear);
    attach::attach_to(runner.state_mut(), mine_on_land, field);

    let library_before = runner.state().players[P0.0 as usize].library.len();
    runner.cast(reverie).target_object(bear).resolve();
    runner.advance_until_stack_empty();

    let library_after = runner.state().players[P0.0 as usize].library.len();
    assert_eq!(
        library_before - library_after,
        3,
        "draws for Sage's Reverie itself, Mine On Bear and Mine On Ogre only"
    );
    let bear_obj = &runner.state().objects[&bear];
    assert_eq!(
        (bear_obj.power, bear_obj.toughness),
        (Some(5), Some(5)),
        "the enchanted creature gets +1/+1 for each of the three"
    );
    assert_eq!(
        runner.state().objects[&ogre].power,
        Some(3),
        "only the enchanted creature gets the bonus"
    );
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
