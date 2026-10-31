//! Cartas mais usadas nos decks da base que o motor deixava fora da cobertura
//! (medição de 30/10/2026 no build `v0.119.0-mf`). Cada teste fixa o trecho que
//! faltava pelo pipeline real de conjurar/ativar/resolver, ou pela habilidade
//! parseada quando o cenário não alcança o gatilho sem custo alto.
//!
//! - Study Hall (CR 903.8): "scry X, where X is the number of times it's been
//!   cast from the command zone this game" não tinha quantidade.
//! - Quartzwood Crasher (CR 120.1 + CR 603.2c): "the amount of damage those
//!   creatures dealt to that player" não tinha quantidade.

use super::rules::run_combat;
use engine::game::commander::record_commander_cast;
use engine::game::scenario::{GameScenario, P0};
use engine::parser::oracle::parse_oracle_text;
use engine::types::ability::Effect;
use engine::types::actions::GameAction;
use engine::types::game_state::WaitingFor;
use engine::types::identifiers::ObjectId;
use engine::types::keywords::Keyword;
use engine::types::mana::{ManaCost, ManaSpellGrant, ManaType, ManaUnit};
use engine::types::phase::Phase;

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
