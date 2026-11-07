//! Field reports 2026-11-07 (MagicFinder mesa), second batch.
//!
//! 1. Prophet of Kruphix — "Untap all creatures and lands you control during
//!    each other player's untap step." — never untapped anything on the
//!    opponent's turn. "creatures and lands you control" parses to an `Or` of
//!    two typed legs, and the Seedborn-class detector only accepted a single
//!    typed subject (CR 502.3).
//! 2. Hydroform ("Target land becomes a 3/3 Elemental creature with flying until
//!    end of turn.") cast on the opponent's turn: the automatic payment tapped
//!    the Island it targeted, so the new 3/3 flier could not block (CR 509.1a).
//!    Targets are chosen before the cost is paid (CR 601.2c, 601.2g-h); the
//!    planner now taps the paid spell's own targets only when nothing else pays.

use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::types::ability::TargetRef;
use engine::types::actions::GameAction;
use engine::types::card_type::CoreType;
use engine::types::game_state::WaitingFor;
use engine::types::identifiers::ObjectId;
use engine::types::mana::{ManaColor, ManaCost, ManaCostShard};
use engine::types::phase::Phase;

const PROPHET: &str = "You may cast creature spells as though they had flash.\n\
Untap all creatures and lands you control during each other player's untap step.";

#[test]
fn prophet_of_kruphix_untaps_creatures_and_lands_on_opponents_untap_step() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::End);
    let prophet = scenario
        .add_creature_from_oracle(P0, "Prophet of Kruphix", 2, 3, PROPHET)
        .id();
    let bear = scenario.add_creature(P0, "Grizzly Bears", 2, 2).id();
    let forest = scenario.add_basic_land(P0, ManaColor::Green);
    let island = scenario.add_basic_land(P0, ManaColor::Blue);
    let mut runner = scenario.build();
    for id in [prophet, bear, forest, island] {
        runner.state_mut().objects.get_mut(&id).unwrap().tapped = true;
    }

    runner.advance_to_upkeep();
    assert_eq!(runner.state().active_player, P1, "reach guard: P1's turn");

    for id in [prophet, bear, forest, island] {
        assert!(
            !runner.state().objects[&id].tapped,
            "CR 502.3: Prophet's controller untaps creatures and lands during each \
             other player's untap step ({})",
            runner.state().objects[&id].name
        );
    }
}

#[test]
fn prophet_of_kruphix_leaves_other_permanent_types_tapped() {
    // Non-vacuity for the compound subject: only creatures and lands untap —
    // an artifact its controller taps stays tapped through the opponent's
    // untap step.
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::End);
    scenario.add_creature_from_oracle(P0, "Prophet of Kruphix", 2, 3, PROPHET);
    let rock = scenario
        .add_artifact_from_oracle(P0, "Inert Relic", "")
        .id();
    let mut runner = scenario.build();
    runner.state_mut().objects.get_mut(&rock).unwrap().tapped = true;

    runner.advance_to_upkeep();
    assert_eq!(runner.state().active_player, P1);
    assert!(
        runner.state().objects[&rock].tapped,
        "Prophet untaps creatures and lands only, not artifacts"
    );
}

const HYDROFORM: &str =
    "Target land becomes a 3/3 Elemental creature with flying until end of turn. It's still a land.";

fn hydroform_board(with_spare_island: bool) -> (GameRunner, ObjectId, ObjectId, Option<ObjectId>) {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    // The targeted Island enters FIRST, so battlefield order alone would pick it.
    let target = scenario.add_basic_land(P0, ManaColor::Blue);
    let forest = scenario.add_basic_land(P0, ManaColor::Green);
    let spare = with_spare_island.then(|| scenario.add_basic_land(P0, ManaColor::Blue));
    let hydroform = scenario
        .add_spell_to_hand_from_oracle(P0, "Hydroform", true, HYDROFORM)
        .with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::Green, ManaCostShard::Blue],
            generic: 0,
        })
        .id();
    let mut runner = scenario.build();
    let card_id = runner.state().objects[&hydroform].card_id;
    runner
        .act(GameAction::CastSpell {
            object_id: hydroform,
            card_id,
            targets: vec![],
            payment_mode: Default::default(),
        })
        .expect("cast Hydroform");
    if matches!(
        runner.state().waiting_for,
        WaitingFor::TargetSelection { .. }
    ) {
        runner
            .act(GameAction::SelectTargets {
                targets: vec![TargetRef::Object(target)],
            })
            .expect("target the first Island");
    }
    (runner, target, forest, spare)
}

#[test]
fn hydroform_auto_payment_does_not_tap_its_target_land() {
    let (mut runner, target, forest, spare) = hydroform_board(true);
    let spare = spare.unwrap();
    assert!(
        runner.state().objects[&forest].tapped,
        "{{G}} from the Forest"
    );
    assert!(
        runner.state().objects[&spare].tapped,
        "{{U}} comes from the Island Hydroform does NOT target"
    );
    assert!(
        !runner.state().objects[&target].tapped,
        "the targeted Island stays untapped so the 3/3 flier can block"
    );
    runner.advance_until_stack_empty();
    let land = &runner.state().objects[&target];
    assert!(
        land.card_types.core_types.contains(&CoreType::Creature),
        "reach guard: Hydroform resolved on the untapped Island"
    );
    assert!(!land.tapped);
}

#[test]
fn hydroform_taps_its_target_when_nothing_else_pays() {
    // Only the targeted Island makes {U}: paying is still legal (CR 601.2g-h)
    // and the deprioritization must not make the spell unpayable.
    let (runner, target, forest, _) = hydroform_board(false);
    assert!(runner.state().objects[&forest].tapped);
    assert!(runner.state().objects[&target].tapped);
    assert!(
        !runner.state().stack.is_empty(),
        "the spell was cast and is on the stack"
    );
}
