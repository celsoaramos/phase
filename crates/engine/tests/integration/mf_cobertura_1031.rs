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
//! - Sage's Reverie (CR 303.4b): "each Aura you control that's attached to a
//!   creature" não tinha filtro; o compra-cartas ficava sem quantidade e o bônus
//!   estático contava também Auras presas a jogadores.
//! - Biophagus (CR 106.6a + CR 614.1c): "If this mana is spent to cast a creature
//!   spell, that creature enters with an additional +1/+1 counter on it" só era
//!   lido na forma do Opal Palace; vale também para Animal Attendant e
//!   Guildmages' Forum.

use super::rules::run_combat;
use engine::game::commander::record_commander_cast;
use engine::game::derived::derive_display_state;
use engine::game::effects::attach::attach_to;
use engine::game::game_object::AttachTarget;
use engine::game::layers::evaluate_layers;
use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::game::scenario_db::GameScenarioDbExt;
use engine::parser::oracle::parse_oracle_text;
use engine::types::ability::Effect;
use engine::types::actions::GameAction;
use engine::types::counter::CounterType;
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

const SAGES_REVERIE: &str = "Enchant creature\nWhen this Aura enters, draw a card for each Aura \
    you control that's attached to a creature.\nEnchanted creature gets +1/+1 for each Aura you \
    control that's attached to a creature.";

/// Auras around the Reverie's host: one of ours on an opponent's creature (counts),
/// one of ours on a player (not attached to a creature), one of the opponent's on
/// our creature (not ours).
fn reverie_board(scenario: &mut GameScenario) -> (ObjectId, [ObjectId; 3]) {
    let host = scenario.add_creature(P0, "Reverie Host", 2, 2).id();
    scenario.add_creature(P1, "Their Creature", 1, 1);
    let mine_on_creature = scenario
        .add_enchantment_from_oracle(P0, "Mine On Creature", "Enchant creature")
        .with_subtypes(vec!["Aura"])
        .id();
    let mine_on_player = scenario
        .add_enchantment_from_oracle(P0, "Mine On Player", "Enchant player")
        .with_subtypes(vec!["Aura", "Curse"])
        .id();
    let their_aura = scenario
        .add_enchantment_from_oracle(P1, "Their Aura", "Enchant creature")
        .with_subtypes(vec!["Aura"])
        .id();
    (host, [mine_on_creature, mine_on_player, their_aura])
}

fn attach_reverie_board(runner: &mut GameRunner, host: ObjectId, auras: [ObjectId; 3]) {
    let theirs = runner
        .state()
        .battlefield
        .iter()
        .copied()
        .find(|id| runner.state().objects[id].name == "Their Creature")
        .expect("opponent creature");
    let [mine_on_creature, mine_on_player, their_aura] = auras;
    attach_to(runner.state_mut(), mine_on_creature, theirs);
    attach_to(runner.state_mut(), their_aura, host);
    runner
        .state_mut()
        .objects
        .get_mut(&mine_on_player)
        .unwrap()
        .attached_to = Some(AttachTarget::Player(P1));
}

#[test]
fn sages_reverie_counts_only_your_auras_attached_to_creatures() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let (host, auras) = reverie_board(&mut scenario);
    let reverie = scenario
        .add_enchantment_from_oracle(P0, "Sage's Reverie", "")
        .with_subtypes(vec!["Aura"])
        .from_oracle_text_with_keywords(&["Enchant creature"], SAGES_REVERIE)
        .id();
    let mut runner = scenario.build();
    attach_reverie_board(&mut runner, host, auras);
    attach_to(runner.state_mut(), reverie, host);
    evaluate_layers(runner.state_mut());
    derive_display_state(runner.state_mut());

    let obj = &runner.state().objects[&host];
    assert_eq!(
        (obj.power, obj.toughness),
        (Some(4), Some(4)),
        "+1/+1 for the Reverie and for our Aura on a creature; the Curse and the \
         opponent's Aura do not count"
    );
}

#[test]
fn sages_reverie_enters_and_draws_for_each_of_your_auras_on_creatures() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let (host, auras) = reverie_board(&mut scenario);
    let reverie = scenario
        .add_creature_to_hand(P0, "Sage's Reverie", 0, 0)
        .as_enchantment()
        .with_subtypes(vec!["Aura"])
        .with_mana_cost(ManaCost::generic(0))
        .from_oracle_text_with_keywords(&["Enchant creature"], SAGES_REVERIE)
        .id();
    scenario.with_library_top(P0, &["Island", "Island", "Island", "Island"]);
    let mut runner = scenario.build();
    attach_reverie_board(&mut runner, host, auras);
    let hand_before = runner.state().players[0].hand.len();

    runner.cast(reverie).target_object(host).resolve();
    runner.advance_until_stack_empty();

    let state = runner.state();
    assert_eq!(state.objects[&reverie].zone, Zone::Battlefield);
    assert_eq!(
        state.players[0].hand.len(),
        hand_before - 1 + 2,
        "draws for the Reverie itself and our Aura on a creature"
    );
}

const BIOPHAGUS: &str = "Genomic Enhancement — {T}: Add one mana of any color. If this mana \
    is spent to cast a creature spell, that creature enters with an additional +1/+1 counter \
    on it.";

#[test]
fn biophagus_mana_spent_on_a_creature_adds_a_counter() {
    let grants = mana_grants(BIOPHAGUS, "Biophagus");
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let biophagus = scenario.add_creature(P0, "Biophagus", 1, 1).id();
    let creature = scenario
        .add_creature_to_hand(P0, "Grown Bear", 2, 2)
        .with_mana_cost(ManaCost::generic(2))
        .id();
    let mut pool = vec![{
        let mut mana = ManaUnit::new(ManaType::Green, biophagus, false, vec![]);
        mana.grants = grants;
        mana
    }];
    pool.push(ManaUnit::new(
        ManaType::Colorless,
        ObjectId(0),
        false,
        vec![],
    ));
    scenario.with_mana_pool(P0, pool);
    let mut runner = scenario.build();

    runner.cast(creature).resolve();
    runner.advance_until_stack_empty();

    let obj = &runner.state().objects[&creature];
    assert_eq!(obj.zone, Zone::Battlefield);
    assert_eq!(
        obj.counters
            .get(&CounterType::Plus1Plus1)
            .copied()
            .unwrap_or(0),
        1,
        "the creature cast with Biophagus mana enters with one +1/+1 counter"
    );
}
