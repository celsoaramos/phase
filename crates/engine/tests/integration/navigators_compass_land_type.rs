//! CR 205.1b + CR 305.7 + CR 611.2a: Navigator's Compass — "{T}: Until end of
//! turn, target land you control becomes the basic land type of your choice in
//! addition to its other types."
//!
//! The parser emits `Choose { BasicLandType }` whose apply-half is a
//! `GenericEffect { affected: ParentTarget, target: <land you control> }`. This
//! row drives the REAL activation: target selection, the type choice, the
//! resolution, then the layer pass, and asserts the land gained the subtype and
//! its intrinsic mana ability (CR 305.6).

use engine::game::layers::evaluate_layers;
use engine::game::scenario::{GameScenario, P0};
use engine::types::actions::GameAction;
use engine::types::ability::TargetRef;
use engine::types::game_state::WaitingFor;
use engine::types::mana::ManaColor;
use engine::types::phase::Phase;

const COMPASS: &str = "When this artifact enters, you gain 3 life.\n{T}: Until end of turn, target land you control becomes the basic land type of your choice in addition to its other types.";

#[test]
fn navigators_compass_adds_the_chosen_basic_land_type_to_the_target() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let compass = scenario
        .add_artifact_from_oracle(P0, "Navigator's Compass", COMPASS)
        .id();
    let mountain = scenario.add_basic_land(P0, ManaColor::Red);
    let mut runner = scenario.build();

    runner
        .act(GameAction::ActivateAbility {
            source_id: compass,
            ability_index: 0,
        })
        .expect("the Compass ability activates");

    let mut chose_type = false;
    for _ in 0..20 {
        let waiting = runner.state().waiting_for.clone();
        let result = match waiting {
            WaitingFor::TargetSelection { .. } => runner.act(GameAction::ChooseTarget {
                target: Some(TargetRef::Object(mountain)),
            }),
            WaitingFor::NamedChoice { .. } => {
                chose_type = true;
                runner.act(GameAction::ChooseOption {
                    choice: "Forest".to_string(),
                })
            }
            WaitingFor::Priority { .. } if runner.state().stack.is_empty() && chose_type => break,
            WaitingFor::Priority { .. } => runner.act(GameAction::PassPriority),
            other => panic!("unexpected prompt: {other:?}"),
        };
        result.expect("the action is accepted");
    }
    assert!(chose_type, "the controller must be asked for a basic land type");

    runner.state_mut().layers_dirty.mark_full();
    evaluate_layers(runner.state_mut());
    let land = &runner.state().objects[&mountain];
    assert!(
        land.card_types.subtypes.iter().any(|s| s == "Forest"),
        "the target must gain Forest; subtypes = {:?}",
        land.card_types.subtypes
    );
    assert!(
        land.card_types.subtypes.iter().any(|s| s == "Mountain"),
        "\"in addition to its other types\": Mountain must stay; subtypes = {:?}",
        land.card_types.subtypes
    );
    // CR 305.6: a land with a basic land type has that type's intrinsic mana
    // ability — the reason anyone activates the Compass.
    let abilities = format!("{:?}", land.abilities);
    assert!(
        abilities.contains("Green"),
        "the Forest type must bring {{T}}: Add {{G}}; abilities = {abilities}"
    );
}

/// Sibling on the creature-type axis of the same `Choose { persist: false }` →
/// `AddChosenSubtype` chain: Mistform Dreamer, "{1}: This creature becomes the
/// creature type of your choice until end of turn."

#[test]
fn mistform_dreamer_becomes_the_chosen_creature_type() {
    const DREAMER: &str = "Flying\n{1}: This creature becomes the creature type of your choice until end of turn.";
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let dreamer = scenario
        .add_creature_from_oracle(P0, "Mistform Dreamer", 2, 1, DREAMER)
        .id();
    scenario.with_mana_pool(P0, vec![engine::types::mana::ManaUnit::new(
        engine::types::mana::ManaType::Colorless, dreamer, false, vec![],
    )]);
    let mut runner = scenario.build();
    runner
        .act(GameAction::ActivateAbility { source_id: dreamer, ability_index: 0 })
        .expect("the Dreamer ability activates");
    let mut chose = false;
    for _ in 0..20 {
        match runner.state().waiting_for.clone() {
            WaitingFor::NamedChoice { .. } => {
                chose = true;
                runner
                    .act(GameAction::ChooseOption { choice: "Dragon".to_string() })
                    .expect("the creature type is accepted");
            }
            WaitingFor::Priority { .. } if runner.state().stack.is_empty() && chose => break,
            WaitingFor::Priority { .. } => {
                runner.act(GameAction::PassPriority).expect("pass");
            }
            WaitingFor::ManaPayment { .. } => {
                runner.act(GameAction::PassPriority).expect("pay from pool");
            }
            other => panic!("unexpected prompt: {other:?}"),
        }
    }
    assert!(chose, "the controller must be asked for a creature type");
    runner.state_mut().layers_dirty.mark_full();
    evaluate_layers(runner.state_mut());
    let subtypes = &runner.state().objects[&dreamer].card_types.subtypes;
    assert!(
        subtypes.iter().any(|s| s == "Dragon"),
        "Mistform Dreamer must become a Dragon; subtypes = {subtypes:?}"
    );
}
