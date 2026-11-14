//! Relatos da mesa de 17/11/2026 (motor v0.125.0).
//!
//! - **Devouring Hellion**: "As this creature enters, you may sacrifice any
//!   number of creatures and/or planeswalkers. If you do, it enters with twice
//!   that many +1/+1 counters on it." — a linha inteira caía no gap
//!   `enters_with_clause` e o Hellion entrava sem perguntar nada.
//! - **Starfleet Communicator**: o equipado ganha "Whenever this creature
//!   attacks, draw X cards, then you may put up to X land cards from your hand
//!   onto the battlefield tapped, where X is the number of other creatures you
//!   control with a Communicator attached to them." — o "where X" não era lido
//!   (o filtro "with a Communicator attached to them" não existia) e ninguém
//!   comprava carta.

use engine::game::combat::AttackTarget;
use engine::game::effects::attach;
use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::types::actions::GameAction;
use engine::types::counter::CounterType;
use engine::types::game_state::WaitingFor;
use engine::types::identifiers::ObjectId;
use engine::types::mana::ManaCost;
use engine::types::phase::Phase;
use engine::types::zones::Zone;

const DEVOURING_HELLION: &str = "As this creature enters, you may sacrifice any number of \
creatures and/or planeswalkers. If you do, it enters with twice that many +1/+1 counters on it.";

fn hellion_scenario() -> (GameScenario, ObjectId, ObjectId, ObjectId) {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let hellion = scenario
        .add_creature_to_hand_from_oracle(P0, "Devouring Hellion", 2, 2, DEVOURING_HELLION)
        .with_mana_cost(ManaCost::zero())
        .id();
    let bear = scenario.add_creature(P0, "Grizzly Bears", 2, 2).id();
    let walker = scenario
        .add_planeswalker_from_oracle(P0, "Chandra, Novice", "Chandra", 3, "")
        .id();
    (scenario, hellion, bear, walker)
}

fn plus_counters(runner: &GameRunner, id: ObjectId) -> u32 {
    runner.state().objects[&id]
        .counters
        .get(&CounterType::Plus1Plus1)
        .copied()
        .unwrap_or(0)
}

#[test]
fn devouring_hellion_enters_with_two_counters_per_permanent_sacrificed() {
    let (scenario, hellion, bear, walker) = hellion_scenario();
    let mut runner = scenario.build();
    runner.cast(hellion).effect_zone(&[bear, walker]).resolve();

    assert_eq!(runner.state().objects[&hellion].zone, Zone::Battlefield);
    assert_eq!(runner.state().objects[&bear].zone, Zone::Graveyard);
    assert_eq!(
        runner.state().objects[&walker].zone,
        Zone::Graveyard,
        "planeswalkers are part of the pool (\"creatures and/or planeswalkers\")"
    );
    assert_eq!(
        plus_counters(&runner, hellion),
        4,
        "two permanents sacrificed → twice that many +1/+1 counters"
    );
}

#[test]
fn devouring_hellion_prompt_excludes_itself_and_decline_enters_without_counters() {
    let (scenario, hellion, bear, walker) = hellion_scenario();
    let mut runner = scenario.build();
    runner.cast(hellion).resolve();

    let WaitingFor::EffectZoneChoice { cards, .. } = &runner.state().waiting_for else {
        panic!(
            "expected the as-enters sacrifice prompt, got {:?}",
            runner.state().waiting_for
        );
    };
    assert!(
        cards.contains(&bear) && cards.contains(&walker),
        "pool={cards:?}"
    );
    assert!(
        !cards.contains(&hellion),
        "the Hellion cannot devour itself; pool={cards:?}"
    );

    runner
        .act(GameAction::SelectCards { cards: vec![] })
        .expect("decline the sacrifice");
    runner.advance_until_stack_empty();

    assert_eq!(runner.state().objects[&hellion].zone, Zone::Battlefield);
    assert_eq!(runner.state().objects[&bear].zone, Zone::Battlefield);
    assert_eq!(runner.state().objects[&walker].zone, Zone::Battlefield);
    assert_eq!(plus_counters(&runner, hellion), 0);
}

const STARFLEET_COMMUNICATOR: &str = "When you cast this spell, copy it. (The copy becomes a \
token.)\nEquipped creature has \"Whenever this creature attacks, draw X cards, then you may put \
up to X land cards from your hand onto the battlefield tapped, where X is the number of other \
creatures you control with a Communicator attached to them.\"\nEquip {1}";

fn communicator(scenario: &mut GameScenario, name: &str) -> ObjectId {
    scenario
        .add_artifact_from_oracle(P0, name, STARFLEET_COMMUNICATOR)
        .with_subtypes(vec!["Communicator", "Equipment"])
        .id()
}

#[test]
fn starfleet_communicator_attack_draws_one_per_other_communicator_creature() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let maralen = scenario.add_creature(P0, "Maralen", 2, 3).id();
    let oona = scenario.add_creature(P0, "Oona", 4, 4).id();
    // A third creature with no Communicator does not count.
    scenario.add_creature(P0, "Plain Bear", 2, 2);
    let comm_a = communicator(&mut scenario, "Starfleet Communicator A");
    let comm_b = communicator(&mut scenario, "Starfleet Communicator B");
    scenario.with_library_top(P0, &["Card A", "Card B", "Card C"]);
    let land = scenario.add_land_to_hand(P0, "Island").id();

    let mut runner = scenario.build();
    attach::attach_to(runner.state_mut(), comm_a, maralen);
    attach::attach_to(runner.state_mut(), comm_b, oona);
    let hand_before = runner.state().players[P0.0 as usize].hand.len();
    let library_before = runner.state().players[P0.0 as usize].library.len();

    runner.pass_both_players();
    runner
        .declare_attackers(&[(maralen, AttackTarget::Player(P1))])
        .expect("Maralen attacks");
    for _ in 0..16 {
        match runner.state().waiting_for.clone() {
            WaitingFor::EffectZoneChoice { cards, .. } => {
                assert!(
                    cards.contains(&land),
                    "the land in hand is offered: {cards:?}"
                );
                runner
                    .act(GameAction::SelectCards { cards: vec![land] })
                    .expect("put the land onto the battlefield");
            }
            WaitingFor::OptionalEffectChoice { .. } => {
                runner
                    .act(GameAction::DecideOptionalEffect { accept: true })
                    .expect("accept putting lands");
            }
            WaitingFor::Priority { .. } if runner.state().stack.is_empty() => break,
            WaitingFor::Priority { .. } => {
                runner.act(GameAction::PassPriority).expect("pass");
            }
            other => panic!("unexpected prompt: {other:?}"),
        }
    }

    let library_after = runner.state().players[P0.0 as usize].library.len();
    let hand_after = runner.state().players[P0.0 as usize].hand.len();
    assert_eq!(
        library_before - library_after,
        1,
        "X = 1 (Oona is the other creature with a Communicator): one card drawn"
    );
    assert_eq!(
        hand_after, hand_before,
        "one card drawn, the land left the hand"
    );
    let land_obj = &runner.state().objects[&land];
    assert_eq!(
        land_obj.zone,
        Zone::Battlefield,
        "up to X = 1 land put onto the battlefield"
    );
    assert!(land_obj.tapped, "the land enters tapped");
}
