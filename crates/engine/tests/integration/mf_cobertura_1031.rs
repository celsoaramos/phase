//! Cartas mais usadas nos decks da base que o motor deixava fora da cobertura
//! (medição de 30/10/2026 no build `v0.119.0-mf`). Cada teste fixa o trecho que
//! faltava pelo pipeline real de conjurar/ativar/resolver, ou pela habilidade
//! parseada quando o cenário não alcança o gatilho sem custo alto.
//!
//! - Study Hall (CR 903.8): "scry X, where X is the number of times it's been
//!   cast from the command zone this game" não tinha quantidade.
//! - Quartzwood Crasher (CR 120.1 + CR 603.2c): "the amount of damage those
//!   creatures dealt to that player" não tinha quantidade.
//! - Nissa's Pilgrimage (CR 614.15): o "instead of two" do Spell mastery troca só
//!   a contagem da busca; revelar, pôr no campo e embaralhar seguem nos dois ramos.

use super::rules::run_combat;
use engine::game::commander::record_commander_cast;
use engine::game::scenario::{GameRunner, GameScenario, P0};
use engine::game::scenario_db::GameScenarioDbExt;
use engine::parser::oracle::parse_oracle_text;
use engine::types::ability::Effect;
use engine::types::actions::GameAction;
use engine::types::game_state::WaitingFor;
use engine::types::identifiers::ObjectId;
use engine::types::keywords::Keyword;
use engine::types::mana::{ManaCost, ManaSpellGrant, ManaType, ManaUnit};
use engine::types::phase::Phase;
use engine::types::zones::Zone;

use super::support::shared_card_db;

fn mana_grants(oracle: &str, name: &str) -> Vec<ManaSpellGrant> {
    let parsed = parse_oracle_text(oracle, name, &[], &["Land".to_string()], &[]);
    parsed
        .abilities
        .iter()
        .find_map(|ability| match &*ability.effect {
            Effect::Mana { grants, .. } if !grants.is_empty() => Some(grants.clone()),
            _ => None,
        })
        .expect("card must parse a mana ability with a spend grant")
}

const STUDY_HALL: &str = "{T}: Add {C}.\n{1}, {T}: Add one mana of any color. When you spend \
    this mana to cast your commander, scry X, where X is the number of times it's been cast \
    from the command zone this game.";

#[test]
fn study_hall_scries_once_per_command_zone_cast_including_this_one() {
    let grants = mana_grants(STUDY_HALL, "Study Hall");
    assert!(
        matches!(grants.as_slice(), [ManaSpellGrant::TriggerOnSpend { .. }]),
        "Study Hall must keep its spend trigger: {grants:?}"
    );

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
    scenario.with_library_top(P0, &["Island", "Island", "Island", "Island", "Island"]);
    let mut pool = vec![{
        let mut mana = ManaUnit::new(ManaType::Blue, hall, false, vec![]);
        mana.grants = grants;
        mana
    }];
    pool.extend((0..4).map(|_| ManaUnit::new(ManaType::Colorless, ObjectId(0), false, vec![])));
    scenario.with_mana_pool(P0, pool);
    let mut runner = scenario.build();
    runner.state_mut().format_config.command_zone = true;
    // Two earlier casts from the command zone; this one makes three.
    record_commander_cast(runner.state_mut(), commander);
    record_commander_cast(runner.state_mut(), commander);

    runner.cast(commander).commit();
    for _ in 0..6 {
        if matches!(runner.state().waiting_for, WaitingFor::ScryChoice { .. }) {
            break;
        }
        runner
            .act(GameAction::PassPriority)
            .expect("pass priority while the spend trigger resolves");
    }
    let WaitingFor::ScryChoice { cards, .. } = runner.state().waiting_for.clone() else {
        panic!(
            "expected the Study Hall scry, got {:?}",
            runner.state().waiting_for
        );
    };
    assert_eq!(cards.len(), 3, "scry X counts the current cast too");
}

const QUARTZWOOD_CRASHER: &str = "Trample\nWhenever one or more creatures you control with \
    trample deal combat damage to a player, create an X/X green Dinosaur Beast creature token \
    with trample, where X is the amount of damage those creatures dealt to that player.";

#[test]
fn quartzwood_crasher_token_is_the_total_trample_damage_to_that_player() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let crasher = scenario
        .add_creature(P0, "Quartzwood Crasher", 6, 6)
        .from_oracle_text_with_keywords(&["Trample"], QUARTZWOOD_CRASHER)
        .id();
    let trampler = scenario
        .add_creature(P0, "Trampling Bear", 2, 2)
        .with_keyword(Keyword::Trample)
        .id();
    // Not trample: its damage must not count toward X.
    let plain = scenario.add_creature(P0, "Plain Bear", 3, 3).id();
    let mut runner = scenario.build();

    run_combat(&mut runner, vec![crasher, trampler, plain], vec![]);
    for _ in 0..10 {
        if runner.state().stack.is_empty() {
            break;
        }
        runner
            .act(GameAction::PassPriority)
            .expect("resolve the Crasher trigger");
    }

    let tokens: Vec<_> = runner
        .state()
        .battlefield
        .iter()
        .map(|id| &runner.state().objects[id])
        .filter(|obj| obj.name == "Dinosaur Beast")
        .map(|obj| (obj.power, obj.toughness))
        .collect();
    assert_eq!(
        tokens,
        vec![(Some(8), Some(8))],
        "one X/X token where X is the trample creatures' damage (6 + 2)"
    );
}

const NISSAS_PILGRIMAGE: &str = "Search your library for up to two basic Forest cards, reveal \
    those cards, and put one onto the battlefield tapped and the rest into your hand. Then \
    shuffle.\nSpell mastery — If there are two or more instant and/or sorcery cards in your \
    graveyard, search your library for up to three basic Forest cards instead of two.";

/// Casts Nissa's Pilgrimage with `spells_in_graveyard` instants in the graveyard,
/// asserts the search size, takes every offered Forest, and returns the runner.
fn cast_pilgrimage(spells_in_graveyard: usize, expected_count: usize) -> Option<GameRunner> {
    let db = shared_card_db()?;
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let pilgrimage = scenario
        .add_spell_to_hand_from_oracle(P0, "Nissa's Pilgrimage", false, NISSAS_PILGRIMAGE)
        .with_mana_cost(ManaCost::generic(0))
        .id();
    for _ in 0..spells_in_graveyard {
        scenario.add_spell_to_graveyard(P0, "Spent Instant", true);
    }
    for _ in 0..4 {
        scenario.add_real_card(P0, "Forest", Zone::Library, db);
    }
    let mut runner = scenario.build();
    engine::game::rehydrate_game_from_card_db(runner.state_mut(), db);

    let outcome = runner.cast(pilgrimage).resolve();
    let WaitingFor::SearchChoice { cards, count, .. } = outcome.final_waiting_for().clone() else {
        panic!(
            "expected the Forest search, got {:?}",
            outcome.final_waiting_for()
        );
    };
    assert_eq!(count, expected_count, "Spell mastery sets the search size");
    runner
        .act(GameAction::SelectCards {
            cards: cards.into_iter().take(expected_count).collect(),
        })
        .expect("take the offered Forests");
    // CR 701.23a: "put one onto the battlefield tapped and the rest into your hand".
    let WaitingFor::SearchPartitionChoice { cards, .. } = runner.state().waiting_for.clone() else {
        panic!(
            "expected the battlefield/hand split, got {:?}",
            runner.state().waiting_for
        );
    };
    assert_eq!(
        cards.len(),
        expected_count,
        "the split sees every found Forest"
    );
    runner
        .act(GameAction::SelectCards {
            cards: vec![cards[0]],
        })
        .expect("choose the Forest that enters");
    runner.advance_until_stack_empty();
    Some(runner)
}

fn forests_in(runner: &GameRunner, zone: Zone) -> usize {
    runner
        .state()
        .objects
        .values()
        .filter(|obj| obj.name == "Forest" && obj.zone == zone && obj.owner == P0)
        .count()
}

#[test]
fn nissas_pilgrimage_spell_mastery_finds_three_and_keeps_the_split() {
    let Some(runner) = cast_pilgrimage(2, 3) else {
        return;
    };
    assert_eq!(
        forests_in(&runner, Zone::Battlefield),
        1,
        "one Forest enters"
    );
    assert_eq!(forests_in(&runner, Zone::Hand), 2, "the rest go to hand");
    assert!(
        runner
            .state()
            .battlefield
            .iter()
            .map(|id| &runner.state().objects[id])
            .filter(|obj| obj.name == "Forest")
            .all(|obj| obj.tapped),
        "the Forest enters tapped"
    );
}

#[test]
fn nissas_pilgrimage_without_spell_mastery_finds_two() {
    let Some(runner) = cast_pilgrimage(1, 2) else {
        return;
    };
    assert_eq!(
        forests_in(&runner, Zone::Battlefield),
        1,
        "one Forest enters"
    );
    assert_eq!(forests_in(&runner, Zone::Hand), 1, "the other goes to hand");
}
