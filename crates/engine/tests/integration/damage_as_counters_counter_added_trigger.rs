//! Wither/infect damage to a creature is a counter placement by the source's
//! controller (CR 120.3d + CR 702.80a + CR 702.90c), so it must reach
//! "whenever you put one or more -1/-1 counters on a creature" triggers
//! (Hapatra, Vizier of Poisons; Nest of Scarabs) and counter replacement
//! effects (CR 614.1a: Vorinclex, Monstrous Raider; Solemnity).
//!
//! Before the fix `apply_damage_after_replacement` wrote the -1/-1 counters
//! straight into `counters`, bypassing `add_counter_with_replacement`: no
//! `CounterAdded { actor }` was emitted and no counter replacement applied. The
//! counters appeared on the creature, but Hapatra never made a Snake.
//!
//! CR 603.2c: the trigger fires once per counter-placement event, not once per
//! counter — three wither damage is ONE Snake for Hapatra, while Nest of Scarabs
//! reads "that many" and makes three Insects.
//!
//! Oracle text below is verbatim from `client/public/card-data.json`.

use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::types::actions::GameAction;
use engine::types::counter::CounterType;
use engine::types::game_state::WaitingFor;
use engine::types::identifiers::ObjectId;
use engine::types::keywords::Keyword;
use engine::types::phase::Phase;
use engine::types::player::PlayerId;
use engine::types::zones::Zone;

use super::rules::AttackTarget;

const HAPATRA_NAME: &str = "Hapatra, Vizier of Poisons";
const HAPATRA: &str = "Whenever Hapatra deals combat damage to a player, you may put a -1/-1 counter on target creature.\nWhenever you put one or more -1/-1 counters on a creature, create a 1/1 green Snake creature token with deathtouch.";
const NEST_OF_SCARABS: &str = "Whenever you put one or more -1/-1 counters on a creature, create that many 1/1 black Insect creature tokens.";
const VORINCLEX: &str = "Trample, haste\nIf you would put one or more counters on a permanent or player, put twice that many of each of those kinds of counters on that permanent or player instead.\nIf an opponent would put one or more counters on a permanent or player, they put half that many of each of those kinds of counters on that permanent or player instead, rounded down.";
const SOLEMNITY: &str = "Players can't get counters.\nCounters can't be put on artifacts, creatures, enchantments, or lands.";

fn tokens(runner: &GameRunner, player: PlayerId, subtype: &str) -> usize {
    runner
        .state()
        .objects
        .values()
        .filter(|o| {
            o.controller == player
                && o.zone == Zone::Battlefield
                && o.card_types
                    .subtypes
                    .iter()
                    .any(|s| s.eq_ignore_ascii_case(subtype))
        })
        .count()
}

fn minus_counters(runner: &GameRunner, id: ObjectId) -> u32 {
    runner
        .state()
        .objects
        .get(&id)
        .and_then(|o| o.counters.get(&CounterType::Minus1Minus1).copied())
        .unwrap_or(0)
}

struct Combat {
    runner: GameRunner,
    blocker: ObjectId,
    defender: PlayerId,
}

/// `attacker_controller` attacks with a 3/3 carrying `keyword`; the other player
/// blocks with an 8/8 that survives even doubled counters (or does not block). `setup` adds the permanents under test.
fn run_combat(
    keyword: Keyword,
    attacker_controller: PlayerId,
    block: bool,
    setup: impl FnOnce(&mut GameScenario),
) -> Combat {
    let defender = if attacker_controller == P0 { P1 } else { P0 };
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    setup(&mut scenario);
    let attacker = scenario
        .add_creature(attacker_controller, "Witherer", 3, 3)
        .with_keyword(keyword)
        .id();
    let blocker = scenario.add_creature(defender, "Blocker", 8, 8).id();
    let mut runner = scenario.build();
    if attacker_controller == P1 {
        runner.state_mut().active_player = P1;
        runner.state_mut().priority_player = P1;
        runner.state_mut().waiting_for = WaitingFor::Priority { player: P1 };
    }
    runner.pass_both_players();
    runner
        .act(GameAction::DeclareAttackers {
            attacks: vec![(attacker, AttackTarget::Player(defender))],
            bands: vec![],
        })
        .expect("DeclareAttackers must be accepted");
    for _ in 0..10 {
        match runner.state().waiting_for {
            WaitingFor::DeclareBlockers { .. } => {
                let blocks = if block {
                    vec![(blocker, attacker)]
                } else {
                    vec![]
                };
                runner
                    .declare_blockers(&blocks)
                    .expect("DeclareBlockers must be accepted");
                break;
            }
            WaitingFor::Priority { .. } => {
                runner.act(GameAction::PassPriority).expect("pass priority");
            }
            ref other => panic!("unexpected prompt before blocks: {other:?}"),
        }
    }
    let _ = runner.combat_damage();
    runner.advance_until_stack_empty();
    Combat {
        runner,
        blocker,
        defender,
    }
}

fn add_hapatra(scenario: &mut GameScenario, player: PlayerId) {
    scenario.add_creature_from_oracle(player, HAPATRA_NAME, 2, 2, HAPATRA);
}

/// CR 120.3d + CR 702.80a + CR 603.2c: three wither combat damage to a creature
/// is one placement by the attacker's controller — exactly one Snake.
#[test]
fn hapatra_triggers_once_on_wither_combat_damage_to_a_creature() {
    let c = run_combat(Keyword::Wither, P0, true, |s| add_hapatra(s, P0));
    assert_eq!(minus_counters(&c.runner, c.blocker), 3);
    assert_eq!(
        tokens(&c.runner, P0, "Snake"),
        1,
        "one damage event = one Hapatra trigger, not one per counter"
    );
}

/// CR 120.3d + CR 702.90c: infect damage to a creature triggers too.
#[test]
fn hapatra_triggers_on_infect_combat_damage_to_a_creature() {
    let c = run_combat(Keyword::Infect, P0, true, |s| add_hapatra(s, P0));
    assert_eq!(minus_counters(&c.runner, c.blocker), 3);
    assert_eq!(tokens(&c.runner, P0, "Snake"), 1);
}

/// CR 120.3b: infect damage to a player gives poison counters — no -1/-1
/// counter is put on a creature, so Hapatra does not trigger. (Guard: passes
/// before the fix as well.)
#[test]
fn infect_damage_to_a_player_gives_poison_without_hapatra_trigger() {
    let c = run_combat(Keyword::Infect, P0, false, |s| add_hapatra(s, P0));
    let defender = c
        .runner
        .state()
        .players
        .iter()
        .find(|p| p.id == c.defender)
        .unwrap();
    assert_eq!(defender.poison_counters, 3);
    assert_eq!(defender.life, 20);
    assert_eq!(tokens(&c.runner, P0, "Snake"), 0);
}

/// CR 120.3d + CR 603.2c: the opponent's wither creature puts the counters, so
/// only the OPPONENT's Hapatra triggers — never the damaged creature's
/// controller's.
#[test]
fn opponent_wither_damage_triggers_only_the_opponents_hapatra() {
    let c = run_combat(Keyword::Wither, P1, true, |s| {
        add_hapatra(s, P0);
        add_hapatra(s, P1);
    });
    assert_eq!(minus_counters(&c.runner, c.blocker), 3);
    assert_eq!(
        tokens(&c.runner, P0, "Snake"),
        0,
        "you did not put the counters"
    );
    assert_eq!(tokens(&c.runner, P1, "Snake"), 1);
}

/// CR 120.3d: noncombat wither damage (the DealDamage effect path) triggers too.
#[test]
fn hapatra_triggers_on_noncombat_wither_damage() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    add_hapatra(&mut scenario, P0);
    let source = {
        let mut b = scenario.add_creature(P0, "Pinger", 1, 1);
        b.from_oracle_text("{T}: This creature deals 2 damage to target creature.");
        b.with_keyword(Keyword::Wither);
        b.id()
    };
    let target = scenario.add_creature(P1, "Target", 5, 5).id();
    let mut runner = scenario.build();
    runner.activate(source, 0).target_object(target).resolve();
    runner.advance_until_stack_empty();
    assert_eq!(minus_counters(&runner, target), 2);
    assert_eq!(runner.state().objects[&target].damage_marked, 0);
    assert_eq!(tokens(&runner, P0, "Snake"), 1);
}

/// Nest of Scarabs: "create that many" reads the placed count from the event.
#[test]
fn nest_of_scarabs_creates_that_many_insects_from_wither_damage() {
    let c = run_combat(Keyword::Wither, P0, true, |s| {
        s.add_enchantment_from_oracle(P0, "Nest of Scarabs", NEST_OF_SCARABS);
    });
    assert_eq!(minus_counters(&c.runner, c.blocker), 3);
    assert_eq!(tokens(&c.runner, P0, "Insect"), 3);
}

/// CR 614.1a: the damage-as-counters placement is subject to counter
/// replacement — Vorinclex doubles the -1/-1 counters its controller puts.
#[test]
fn vorinclex_doubles_wither_damage_counters() {
    let c = run_combat(Keyword::Wither, P0, true, |s| {
        s.add_creature_from_oracle(P0, "Vorinclex, Monstrous Raider", 6, 6, VORINCLEX);
        add_hapatra(s, P0);
    });
    assert_eq!(minus_counters(&c.runner, c.blocker), 6);
    assert_eq!(tokens(&c.runner, P0, "Snake"), 1);
}

/// CR 614.1a: Solemnity stops the -1/-1 counters, so nothing is put and Hapatra
/// does not trigger; the damage is still not marked (CR 702.80a).
#[test]
fn solemnity_prevents_wither_damage_counters() {
    let c = run_combat(Keyword::Wither, P0, true, |s| {
        s.add_enchantment_from_oracle(P1, "Solemnity", SOLEMNITY);
        add_hapatra(s, P0);
    });
    assert_eq!(minus_counters(&c.runner, c.blocker), 0);
    assert_eq!(c.runner.state().objects[&c.blocker].damage_marked, 0);
    assert_eq!(tokens(&c.runner, P0, "Snake"), 0);
}
