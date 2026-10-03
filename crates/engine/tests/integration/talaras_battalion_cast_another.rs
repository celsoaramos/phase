//! "Cast this spell only if you've cast another [<filter>] spell this turn."
//!
//! CR 601.3: the restriction is checked when the spell would be cast — before
//! it is recorded in this turn's spell history. The condition used to be
//! `SpellsCastThisTurn >= 2` ("counting this spell"), so with one earlier spell
//! the count was 1 and the card was never castable.
//!
//! Field report (2026-10-03, Mesa): Talara's Battalion stayed uncastable after
//! Pugnacious Hammerskull (green) had been cast that turn. Illusory Angel
//! ("another spell", no filter) had the same defect.

use engine::game::scenario::{GameRunner, GameScenario, P0};
use engine::types::actions::GameAction;
use engine::types::game_state::CastPaymentMode;
use engine::types::identifiers::ObjectId;
use engine::types::mana::{ManaColor, ManaCost, ManaCostShard};
use engine::types::phase::Phase;
use engine::types::zones::Zone;

// Verbatim Oracle text (card-data.json).
const TALARA: &str = "Trample\nCast this spell only if you've cast another green spell this turn.";
const ILLUSORY_ANGEL: &str = "Flying\nCast this spell only if you've cast another spell this turn.";

fn cast_action(runner: &GameRunner, id: ObjectId) -> GameAction {
    GameAction::CastSpell {
        object_id: id,
        card_id: runner.state().objects[&id].card_id,
        targets: vec![],
        payment_mode: CastPaymentMode::Auto,
    }
}

fn creature_in_hand(
    scenario: &mut GameScenario,
    name: &str,
    color: ManaColor,
    shard: ManaCostShard,
    oracle: &str,
) -> ObjectId {
    let mut b = scenario.add_creature_to_hand(P0, name, 2, 2);
    b.with_mana_cost(ManaCost::Cost {
        shards: vec![shard],
        generic: 1,
    });
    b.with_color(vec![color]);
    if !oracle.is_empty() {
        b.from_oracle_text(oracle);
    }
    b.id()
}

/// Lands for both colours, a green and a blue two-drop, and the restricted card.
fn setup(
    restricted_oracle: &str,
    restricted_color: ManaColor,
) -> (GameRunner, ObjectId, ObjectId, ObjectId) {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    for _ in 0..4 {
        scenario.add_basic_land(P0, ManaColor::Green);
        scenario.add_basic_land(P0, ManaColor::Blue);
    }
    let shard = if restricted_color == ManaColor::Green {
        ManaCostShard::Green
    } else {
        ManaCostShard::Blue
    };
    let restricted = creature_in_hand(
        &mut scenario,
        "Restricted",
        restricted_color,
        shard,
        restricted_oracle,
    );
    let green = creature_in_hand(
        &mut scenario,
        "Green Bear",
        ManaColor::Green,
        ManaCostShard::Green,
        "",
    );
    let blue = creature_in_hand(
        &mut scenario,
        "Blue Bear",
        ManaColor::Blue,
        ManaCostShard::Blue,
        "",
    );
    (scenario.build(), restricted, green, blue)
}

#[test]
fn talara_is_not_castable_before_any_spell() {
    let (mut runner, talara, _, _) = setup(TALARA, ManaColor::Green);
    let action = cast_action(&runner, talara);
    assert!(runner.act(action).is_err(), "no spell cast yet this turn");
    assert_eq!(runner.state().objects[&talara].zone, Zone::Hand);
}

#[test]
fn talara_is_castable_after_another_green_spell() {
    let (mut runner, talara, green, _) = setup(TALARA, ManaColor::Green);
    runner.cast(green).resolve();
    let action = cast_action(&runner, talara);
    runner
        .act(action)
        .expect("CR 601.3: one earlier green spell satisfies \"another green spell\"");
    runner.advance_until_stack_empty();
    assert_eq!(runner.state().objects[&talara].zone, Zone::Battlefield);
}

#[test]
fn talara_is_not_castable_after_only_a_blue_spell() {
    let (mut runner, talara, _, blue) = setup(TALARA, ManaColor::Green);
    runner.cast(blue).resolve();
    let action = cast_action(&runner, talara);
    assert!(
        runner.act(action).is_err(),
        "a blue spell is not \"another green spell\""
    );
    assert_eq!(runner.state().objects[&talara].zone, Zone::Hand);
}

#[test]
fn illusory_angel_is_castable_after_any_other_spell() {
    let (mut runner, angel, _, blue) = setup(ILLUSORY_ANGEL, ManaColor::Blue);
    let refused = cast_action(&runner, angel);
    assert!(runner.act(refused).is_err(), "no spell cast yet this turn");
    runner.cast(blue).resolve();
    let action = cast_action(&runner, angel);
    runner
        .act(action)
        .expect("CR 601.3: one earlier spell satisfies \"another spell\"");
    runner.advance_until_stack_empty();
    assert_eq!(runner.state().objects[&angel].zone, Zone::Battlefield);
}
