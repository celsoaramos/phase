//! Gift on PERMANENT spells. CR 702.174b: on a permanent the gift is given
//! "when this permanent enters, if its gift was promised". The delivery used to
//! be wrapped around `abilities[0]` like an instant's, so Scrapshooter (no
//! abilities) never gave the card and Starforged Sword's Equip ended up nested
//! under a spell-kind wrapper and could not be activated.

use engine::ai_support::legal_actions;
use engine::game::scenario::{GameScenario, P0, P1};
use engine::types::actions::GameAction;
use engine::types::mana::{ManaType, ManaUnit};
use engine::types::phase::Phase;
use engine::types::zones::Zone;

const SCRAPSHOOTER: &str =
    "Gift a card (You may promise an opponent a gift as you cast this spell. \
If you do, when it enters, they draw a card.)\nReach\n\
When this creature enters, if the gift was promised, destroy target artifact or enchantment \
an opponent controls.";

fn board() -> (
    GameScenario,
    engine::types::ObjectId,
    engine::types::ObjectId,
    engine::types::ObjectId,
) {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let draw = scenario.add_card_to_library_top(P1, "Gift Card");
    let relic = scenario
        .add_artifact_from_oracle(P1, "Opponent Relic", "{T}: Add {C}.")
        .id();
    let shooter = scenario
        .add_creature_to_hand(P0, "Scrapshooter", 4, 4)
        .from_oracle_text_with_keywords(&["Gift", "Reach"], SCRAPSHOOTER)
        .id();
    (scenario, draw, relic, shooter)
}

#[test]
fn scrapshooter_promised_gift_draws_for_opponent_and_destroys_their_artifact() {
    let (scenario, draw, relic, shooter) = board();
    let mut runner = scenario.build();
    runner.cast(shooter).accept_optional().resolve();
    runner.advance_until_stack_empty();

    assert_eq!(runner.state().objects[&shooter].zone, Zone::Battlefield);
    assert!(
        runner.state().players[1].hand.contains(&draw),
        "promised Gift a card must draw for the opponent when the creature enters"
    );
    assert_eq!(
        runner.state().objects[&relic].zone,
        Zone::Graveyard,
        "the promised-gift ETB must destroy the opponent's artifact"
    );
}

#[test]
fn scrapshooter_without_gift_neither_draws_nor_destroys() {
    let (scenario, draw, relic, shooter) = board();
    let mut runner = scenario.build();
    let p1_hand = runner.state().players[1].hand.len();
    runner.cast(shooter).decline_optional().resolve();
    runner.advance_until_stack_empty();

    assert_eq!(runner.state().objects[&shooter].zone, Zone::Battlefield);
    assert_eq!(runner.state().players[1].hand.len(), p1_hand);
    assert!(!runner.state().players[1].hand.contains(&draw));
    assert_eq!(runner.state().objects[&relic].zone, Zone::Battlefield);
}

const STARFORGED_SWORD: &str =
    "Gift a tapped Fish (You may promise an opponent a gift as you cast \
this spell. If you do, when it enters, they create a tapped 1/1 blue Fish creature token.)\n\
When this Equipment enters, if the gift was promised, attach this Equipment to target creature \
you control.\n\
Equipped creature gets +3/+3 and loses flying.\n\
Equip {3}";

fn sword_board() -> (
    GameScenario,
    engine::types::ObjectId,
    engine::types::ObjectId,
) {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.with_mana_pool(
        P0,
        (0..3)
            .map(|_| {
                ManaUnit::new(
                    ManaType::Colorless,
                    engine::types::ObjectId(0),
                    false,
                    vec![],
                )
            })
            .collect(),
    );
    let bear = scenario.add_creature(P0, "Bear", 2, 2).id();
    let sword = scenario
        .add_artifact_to_hand_from_oracle(P0, "Starforged Sword", "")
        .with_subtypes(vec!["Equipment"])
        .from_oracle_text_with_keywords(&["Gift", "Equip"], STARFORGED_SWORD)
        .id();
    (scenario, bear, sword)
}

fn fish_for(
    runner: &engine::game::scenario::GameRunner,
    player: engine::types::player::PlayerId,
) -> Vec<engine::types::ObjectId> {
    runner
        .state()
        .objects
        .values()
        .filter(|o| o.zone == Zone::Battlefield && o.controller == player && o.name == "Fish")
        .map(|o| o.id)
        .collect()
}

fn can_equip(runner: &engine::game::scenario::GameRunner, sword: engine::types::ObjectId) -> bool {
    legal_actions(runner.state())
        .iter()
        .any(|a| matches!(a, GameAction::ActivateAbility { source_id, .. } if *source_id == sword))
}

/// CR 702.174b: Starforged Sword's promised gift is a tapped Fish for the
/// opponent when it enters, and Equip stays an activated ability of the
/// Equipment (it used to be nested under a spell-kind gift wrapper).
#[test]
fn starforged_sword_promised_gift_makes_a_tapped_fish_and_equip_stays_activatable() {
    let (scenario, bear, sword) = sword_board();
    let mut runner = scenario.build();
    runner.cast(sword).accept_optional().resolve();
    runner.advance_until_stack_empty();

    assert_eq!(runner.state().objects[&sword].zone, Zone::Battlefield);
    let fish = fish_for(&runner, P1);
    assert_eq!(fish.len(), 1, "the opponent gets exactly one Fish");
    assert!(
        runner.state().objects[&fish[0]].tapped,
        "the gifted Fish enters tapped"
    );
    assert_eq!(
        runner.state().objects[&sword].attached_to,
        Some(bear.into()),
        "the promised-gift ETB attaches the Equipment"
    );
    assert!(can_equip(&runner, sword), "Equip {{3}} must be activatable");
}

#[test]
fn starforged_sword_without_gift_makes_no_fish_and_can_still_equip() {
    let (scenario, _bear, sword) = sword_board();
    let mut runner = scenario.build();
    runner.cast(sword).decline_optional().resolve();
    runner.advance_until_stack_empty();

    assert_eq!(runner.state().objects[&sword].zone, Zone::Battlefield);
    assert!(fish_for(&runner, P1).is_empty());
    assert!(can_equip(&runner, sword), "Equip {{3}} must be activatable");
}
