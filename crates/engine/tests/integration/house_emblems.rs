//! House-rule emblems (`FormatConfig::house_emblems`, MagicFinder "regra da
//! semana"): each Oracle text becomes a game-start command-zone emblem
//! (CR 114.1 / CR 114.4) whose static ability applies to every creature on
//! both sides — including creatures and tokens that enter later.
//!
//! One combat test per season text proves the keyword actually changes play,
//! plus ingress tests proving an unparseable text fails the config instead of
//! becoming an empty emblem that plays a normal game.

use engine::game::combat::AttackTarget;
use engine::game::deck_loading::{load_deck_into_state, DeckPayload};
use engine::game::house_emblems::grant_house_emblems;
use engine::game::layers::evaluate_layers;
use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::types::actions::GameAction;
use engine::types::format::FormatConfig;
use engine::types::game_state::{GameState, WaitingFor};
use engine::types::identifiers::ObjectId;
use engine::types::keywords::Keyword;
use engine::types::phase::Phase;
use engine::types::zones::Zone;

use super::rules::run_combat;

/// Install the house emblems on a built runner and recompute layers, exactly
/// as game start does (`load_deck_into_state` → `grant_house_emblems`).
fn with_house_emblem(runner: &mut GameRunner, text: &str) {
    runner.state_mut().format_config.house_emblems = vec![text.to_string()];
    grant_house_emblems(runner.state_mut());
    evaluate_layers(runner.state_mut());
}

fn has(runner: &GameRunner, id: ObjectId, kw: &Keyword) -> bool {
    runner.state().objects.get(&id).unwrap().has_keyword(kw)
}

fn zone(runner: &GameRunner, id: ObjectId) -> Option<Zone> {
    runner.state().objects.get(&id).map(|o| o.zone)
}

/// CR 702.19b: "All creatures have trample." — a 5/5 blocked by a 2/2
/// assigns lethal to the blocker and the excess 3 to the defending player.
#[test]
fn house_trample_assigns_excess_to_player() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let attacker = scenario.add_creature(P0, "Big", 5, 5).id();
    let blocker = scenario.add_creature(P1, "Small", 2, 2).id();
    let mut runner = scenario.build();
    with_house_emblem(&mut runner, "All creatures have trample.");
    assert!(has(&runner, attacker, &Keyword::Trample));
    assert!(
        has(&runner, blocker, &Keyword::Trample),
        "opponent's creature too"
    );

    run_combat(&mut runner, vec![attacker], vec![(blocker, attacker)]);
    assert_eq!(runner.life(P1), 17, "5 power - 2 lethal = 3 tramples over");
}

/// CR 702.2b: "All creatures have deathtouch." — the OPPONENT's 1/1 blocker
/// destroys a 5/5 attacker with 1 damage.
#[test]
fn house_deathtouch_kills_with_one_damage() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let attacker = scenario.add_creature(P0, "Big", 5, 5).id();
    let blocker = scenario.add_creature(P1, "Tiny", 1, 1).id();
    let mut runner = scenario.build();
    with_house_emblem(&mut runner, "All creatures have deathtouch.");

    run_combat(&mut runner, vec![attacker], vec![(blocker, attacker)]);
    runner.advance_until_stack_empty();
    assert_eq!(
        zone(&runner, attacker),
        Some(Zone::Graveyard),
        "CR 704.5h: 1 deathtouch damage destroys the 5/5"
    );
}

/// CR 702.15b: "All creatures have lifelink." — combat damage dealt by the
/// attacker gains its controller that much life.
#[test]
fn house_lifelink_gains_life_from_combat_damage() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let attacker = scenario.add_creature(P0, "Bear", 3, 3).id();
    let mut runner = scenario.build();
    with_house_emblem(&mut runner, "All creatures have lifelink.");

    run_combat(&mut runner, vec![attacker], vec![]);
    assert_eq!(runner.life(P1), 17);
    assert_eq!(runner.life(P0), 23, "lifelink: P0 gains 3");
}

/// CR 702.10b + CR 302.6: "All creatures have haste." — a creature that came
/// under its controller's control this turn can attack.
#[test]
fn house_haste_lets_summoning_sick_creature_attack() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let attacker = scenario
        .add_creature(P0, "Fresh", 2, 2)
        .with_summoning_sickness()
        .id();
    let mut runner = scenario.build();
    with_house_emblem(&mut runner, "All creatures have haste.");
    assert!(has(&runner, attacker, &Keyword::Haste));

    run_combat(&mut runner, vec![attacker], vec![]);
    assert_eq!(runner.life(P1), 18, "the summoning-sick creature attacked");
}

/// Control for the haste test: without the emblem the same attack is illegal.
#[test]
fn without_house_haste_summoning_sick_creature_cannot_attack() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let attacker = scenario
        .add_creature(P0, "Fresh", 2, 2)
        .with_summoning_sickness()
        .id();
    let mut runner = scenario.build();
    runner.pass_both_players();
    let result = runner.act(GameAction::DeclareAttackers {
        attacks: vec![(attacker, AttackTarget::Player(P1))],
        bands: vec![],
    });
    assert!(result.is_err(), "summoning-sick creature must not attack");
}

/// CR 702.9b: "All creatures have flying." — the opponent's vanilla 2/2 can
/// block a flier because it has flying too; without the emblem that block is
/// illegal (control case below).
#[test]
fn house_flying_lets_every_creature_block_fliers() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let attacker = scenario.add_creature(P0, "Bird", 2, 2).flying().id();
    let blocker = scenario.add_creature(P1, "Bear", 2, 2).id();
    let mut runner = scenario.build();
    with_house_emblem(&mut runner, "All creatures have flying.");
    assert!(has(&runner, blocker, &Keyword::Flying));

    run_combat(&mut runner, vec![attacker], vec![(blocker, attacker)]);
    assert_eq!(runner.life(P1), 20, "the flier was blocked");
    runner.advance_until_stack_empty();
    assert_eq!(zone(&runner, blocker), Some(Zone::Graveyard));
}

#[test]
fn without_house_flying_ground_creature_cannot_block_flier() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let attacker = scenario.add_creature(P0, "Bird", 2, 2).flying().id();
    let blocker = scenario.add_creature(P1, "Bear", 2, 2).id();
    let mut runner = scenario.build();
    runner.pass_both_players();
    runner
        .act(GameAction::DeclareAttackers {
            attacks: vec![(attacker, AttackTarget::Player(P1))],
            bands: vec![],
        })
        .unwrap();
    if matches!(runner.state().waiting_for, WaitingFor::Priority { .. }) {
        runner.pass_both_players();
    }
    let result = runner.act(GameAction::DeclareBlockers {
        assignments: vec![(blocker, attacker)],
    });
    assert!(
        result.is_err(),
        "CR 702.9b: ground creature can't block a flier"
    );
}

/// CR 702.20b: "All creatures have vigilance." — attacking doesn't tap.
#[test]
fn house_vigilance_attacker_stays_untapped() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let attacker = scenario.add_creature(P0, "Guard", 2, 2).id();
    let mut runner = scenario.build();
    with_house_emblem(&mut runner, "All creatures have vigilance.");

    run_combat(&mut runner, vec![attacker], vec![]);
    assert_eq!(runner.life(P1), 18);
    assert!(
        !runner.state().objects.get(&attacker).unwrap().tapped,
        "vigilance: the attacker did not tap"
    );
}

/// CR 702.4b + CR 510.4: "All creatures have double strike." — an unblocked
/// 2/2 deals combat damage in both damage steps.
#[test]
fn house_double_strike_deals_damage_twice() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let attacker = scenario.add_creature(P0, "Duelist", 2, 2).id();
    let mut runner = scenario.build();
    with_house_emblem(&mut runner, "All creatures have double strike.");

    run_combat(&mut runner, vec![attacker], vec![]);
    runner.advance_until_stack_empty();
    // Drive through the regular combat damage step if the harness stopped
    // after first-strike damage.
    for _ in 0..4 {
        if runner.life(P1) == 16 {
            break;
        }
        if matches!(runner.state().waiting_for, WaitingFor::Priority { .. }) {
            runner.pass_both_players();
        }
    }
    assert_eq!(runner.life(P1), 16, "2 first-strike + 2 regular damage");
}

/// CR 611.3a: the emblem's effect also covers a TOKEN created after the
/// emblem exists — it enters with haste and attacks that same turn.
#[test]
fn house_emblem_applies_to_token_created_later() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let spell = scenario
        .add_spell_to_hand_from_oracle(
            P0,
            "Muster",
            false,
            "Create a 1/1 white Soldier creature token.",
        )
        .id();
    let mut runner = scenario.build();
    with_house_emblem(&mut runner, "All creatures have haste.");
    runner.cast(spell).resolve();
    evaluate_layers(runner.state_mut());

    let token = runner
        .state()
        .battlefield
        .iter()
        .copied()
        .find(|id| runner.state().objects.get(id).is_some_and(|o| o.is_token))
        .expect("the token was created");
    assert!(has(&runner, token, &Keyword::Haste));

    run_combat(&mut runner, vec![token], vec![]);
    assert_eq!(
        runner.life(P1),
        19,
        "the fresh token attacked thanks to haste"
    );
}

/// Game start: `load_deck_into_state` puts one emblem per text in the command
/// zone, and the emblem carries the parsed static.
#[test]
fn game_start_creates_one_command_zone_emblem_per_text() {
    let mut config = FormatConfig::standard();
    config.house_emblems = vec![
        "All creatures have trample.".to_string(),
        "All creatures have flying.".to_string(),
    ];
    let mut state = GameState::new(config, 2, 7);
    load_deck_into_state(&mut state, &DeckPayload::default());
    let emblems: Vec<_> = state
        .objects
        .values()
        .filter(|o| o.is_emblem && o.zone == Zone::Command)
        .collect();
    assert_eq!(emblems.len(), 2);
    assert!(emblems.iter().all(|e| !e.static_definitions.is_empty()));
}

/// Ingress: a format config whose house emblem the parser doesn't understand
/// must fail deserialization (and with it game initialization).
#[test]
fn format_config_rejects_unparseable_house_emblem() {
    let mut value = serde_json::to_value(FormatConfig::standard()).unwrap();
    value["house_emblems"] = serde_json::json!(["Flibbertigibbet the wombat."]);
    let err = serde_json::from_value::<FormatConfig>(value).unwrap_err();
    assert!(
        err.to_string().contains("house_emblems"),
        "clear error naming the field: {err}"
    );
}

#[test]
fn format_config_house_emblems_roundtrip_and_default() {
    // Legacy payload (no field) still deserializes, with no emblems.
    let legacy = serde_json::to_value(FormatConfig::standard()).unwrap();
    assert!(
        legacy.get("house_emblems").is_none(),
        "empty list is not serialized"
    );
    let parsed: FormatConfig = serde_json::from_value(legacy).unwrap();
    assert!(parsed.house_emblems.is_empty());

    let mut value = serde_json::to_value(FormatConfig::standard()).unwrap();
    value["house_emblems"] = serde_json::json!(["All creatures have double strike."]);
    let parsed: FormatConfig = serde_json::from_value(value).unwrap();
    assert_eq!(
        parsed.house_emblems,
        vec!["All creatures have double strike."]
    );
}
