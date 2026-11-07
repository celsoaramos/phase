//! CR 614.1a: Jinnie Fay, Jetmir's Second — "you may instead create that many
//! 2/2 green Cat creature tokens … or that many 3/1 green Dog creature tokens".
//!
//! Field report (2026-11-07): Lorehold Command's "Create a 3/2 Spirit" mode with
//! Jinnie on the battlefield; accepting the swap for a Cat created no token at
//! all. The applier zeroes the replaced event, and "that many" had no count to
//! read for a spell-created event (only the Moonlit copy substitution latched
//! it), so every branch created zero tokens.

use engine::database::card_db::CardDatabase;
use engine::game::game_object::GameObject;
use engine::game::scenario::{GameRunner, GameScenario, P0};
use engine::game::scenario_db::GameScenarioDbExt;
use engine::types::actions::GameAction;
use engine::types::game_state::WaitingFor;
use engine::types::identifiers::ObjectId;
use engine::types::mana::{ManaType, ManaUnit};
use engine::types::phase::Phase;
use engine::types::zones::Zone;

use crate::support::shared_card_db;

const JINNIE: &str = "Jinnie Fay, Jetmir's Second";
const LOREHOLD_COMMAND: &str = "Choose two —\n\
    • Create a 3/2 red and white Spirit creature token.\n\
    • Creatures you control get +1/+0 and gain indestructible and haste until end of turn.\n\
    • Lorehold Command deals 3 damage to any target. Target player gains 3 life.\n\
    • Sacrifice a permanent, then draw two cards.";

fn card_db() -> &'static CardDatabase {
    shared_card_db().expect("integration card fixture must load")
}

fn setup() -> (GameRunner, ObjectId) {
    let db = card_db();
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let mut pool: Vec<ManaUnit> = (0..3)
        .map(|_| ManaUnit::new(ManaType::Colorless, ObjectId(0), false, vec![]))
        .collect();
    pool.push(ManaUnit::new(ManaType::Red, ObjectId(0), false, vec![]));
    pool.push(ManaUnit::new(ManaType::White, ObjectId(0), false, vec![]));
    scenario.with_mana_pool(P0, pool);
    scenario.add_real_card(P0, JINNIE, Zone::Battlefield, db);
    let command = scenario
        .add_spell_to_hand_from_oracle(P0, "Lorehold Command", true, LOREHOLD_COMMAND)
        .id();
    (scenario.build(), command)
}

fn p0_tokens(runner: &GameRunner) -> Vec<&GameObject> {
    runner
        .state()
        .battlefield
        .iter()
        .filter_map(|id| runner.state().objects.get(id))
        .filter(|obj| obj.is_token && obj.controller == P0)
        .collect()
}

fn has_subtype(obj: &GameObject, subtype: &str) -> bool {
    obj.card_types.subtypes.iter().any(|s| s == subtype)
}

fn drive(runner: &mut GameRunner, accept: bool) {
    for _ in 0..24 {
        match runner.state().waiting_for.clone() {
            WaitingFor::ReplacementChoice { .. } => {
                runner
                    .act(GameAction::ChooseReplacement {
                        index: if accept { 0 } else { 1 },
                    })
                    .expect("answer Jinnie");
            }
            WaitingFor::ChooseOneOfBranch { .. } => {
                runner
                    .act(GameAction::ChooseBranch { index: 0 })
                    .expect("choose Cat");
            }
            WaitingFor::Priority { .. } if !runner.state().stack.is_empty() => {
                runner.act(GameAction::PassPriority).expect("pass");
            }
            _ => return,
        }
    }
}

#[test]
fn lorehold_command_token_mode_with_jinnie_accept_creates_a_cat() {
    let (mut runner, command) = setup();
    drop(runner.cast(command).modes(&[0, 1]).commit());
    drive(&mut runner, true);
    let tokens = p0_tokens(&runner);
    assert_eq!(tokens.len(), 1, "one Cat replaces the one Spirit");
    assert!(has_subtype(tokens[0], "Cat"));
}

#[test]
fn lorehold_command_token_mode_with_jinnie_decline_creates_the_spirit() {
    let (mut runner, command) = setup();
    drop(runner.cast(command).modes(&[0, 1]).commit());
    drive(&mut runner, false);
    let tokens = p0_tokens(&runner);
    assert_eq!(tokens.len(), 1);
    assert!(has_subtype(tokens[0], "Spirit"));
}

/// CR 614.1a: "that many" is the replaced event's count — Raise the Alarm's two
/// Soldiers become two Cats, not zero and not one.
#[test]
fn jinnie_accept_creates_that_many_cats_for_raise_the_alarm() {
    let db = card_db();
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.with_mana_pool(
        P0,
        (0..2)
            .map(|_| ManaUnit::new(ManaType::White, ObjectId(0), false, vec![]))
            .collect(),
    );
    scenario.add_real_card(P0, JINNIE, Zone::Battlefield, db);
    let alarm = scenario.add_real_card(P0, "Raise the Alarm", Zone::Hand, db);
    let mut runner = scenario.build();
    drop(runner.cast(alarm).commit());
    drive(&mut runner, true);
    let tokens = p0_tokens(&runner);
    assert_eq!(tokens.len(), 2, "two Cats replace the two Soldiers");
    assert!(tokens
        .iter()
        .all(|t| has_subtype(t, "Cat") && t.power == Some(2) && t.toughness == Some(2)));
}
