//! Akiri, Fearless Voyager — "{W}: You may unattach an Equipment from a
//! creature you control. If you do, tap that creature and it gains
//! indestructible until end of turn."
//!
//! The unattach verb was unparsed (`Unimplemented`), so paying {W} did
//! nothing, and "it gains indestructible" was bound to Akiri herself instead
//! of the creature the Equipment came off of. The Equipment is chosen as the
//! ability resolves (Akiri ruling 2020-09-25) — nothing is targeted — and
//! "that creature" / "it" name the creature it was unattached from (CR 608.2c).

use engine::game::game_object::AttachTarget;
use engine::types::ability::TargetRef;
use engine::types::actions::GameAction;
use engine::types::keywords::Keyword;
use engine::types::mana::ManaColor;

use super::rules::{GameRunner, GameScenario, ObjectId, Phase, WaitingFor, P0, P1};

const AKIRI: &str = "Whenever you attack a player with one or more equipped creatures, draw a card.\n\
    {W}: You may unattach an Equipment from a creature you control. If you do, tap that creature and it gains indestructible until end of turn.";

const BLADE: &str = "Equipped creature gets +2/+0.\nEquip {1}";

struct Board {
    runner: GameRunner,
    akiri: ObjectId,
    bearer: ObjectId,
    blade: ObjectId,
    rival_blade: ObjectId,
}

fn board() -> Board {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let akiri = scenario
        .add_creature_from_oracle(P0, "Akiri, Fearless Voyager", 3, 3, AKIRI)
        .id();
    let bearer = scenario.add_creature(P0, "Bearer", 2, 2).id();
    let blade = scenario
        .add_artifact_from_oracle(P0, "Blade", BLADE)
        .with_subtypes(vec!["Equipment"])
        .id();
    // An Equipment on an OPPONENT's creature is not "from a creature you control".
    let rival = scenario.add_creature(P1, "Rival", 2, 2).id();
    let rival_blade = scenario
        .add_artifact_from_oracle(P1, "Rival Blade", BLADE)
        .with_subtypes(vec!["Equipment"])
        .id();
    scenario.add_basic_land(P0, ManaColor::White);
    let mut runner = scenario.build();
    attach(&mut runner, blade, bearer);
    attach(&mut runner, rival_blade, rival);
    Board {
        runner,
        akiri,
        bearer,
        blade,
        rival_blade,
    }
}

fn attach(runner: &mut GameRunner, equipment: ObjectId, host: ObjectId) {
    let state = runner.state_mut();
    state.objects.get_mut(&equipment).unwrap().attached_to = Some(AttachTarget::Object(host));
    state
        .objects
        .get_mut(&host)
        .unwrap()
        .attachments
        .push(equipment);
    state.layers_dirty.mark_full();
}

/// Activate {W} and drive the resolution, answering the "you may" with
/// `accept` and the Equipment choice with `pick` (None = whatever is offered
/// first). Returns the eligible set the choice offered, if one was raised.
fn activate(b: &mut Board, accept: bool, pick: Option<ObjectId>) -> Option<Vec<TargetRef>> {
    let runner = &mut b.runner;
    runner
        .act(GameAction::ActivateAbility {
            source_id: b.akiri,
            ability_index: 0,
        })
        .expect("activating Akiri's {W} ability must succeed");
    let mut offered = None;
    for _ in 0..40 {
        match runner.state().waiting_for.clone() {
            WaitingFor::OptionalEffectChoice { .. } => {
                runner
                    .act(GameAction::DecideOptionalEffect { accept })
                    .expect("answer the optional unattach");
            }
            WaitingFor::ChooseObjectsSelection { eligible, .. } => {
                let choice = pick
                    .map(TargetRef::Object)
                    .unwrap_or_else(|| eligible[0].clone());
                offered = Some(eligible);
                runner
                    .act(GameAction::SelectTargets {
                        targets: vec![choice],
                    })
                    .expect("choose the Equipment");
            }
            _ => {
                runner.advance_until_stack_empty();
                if runner.state().stack.is_empty()
                    && matches!(runner.state().waiting_for, WaitingFor::Priority { .. })
                {
                    break;
                }
            }
        }
    }
    offered
}

fn tapped(b: &Board, id: ObjectId) -> bool {
    b.runner.state().objects[&id].tapped
}

fn indestructible(b: &Board, id: ObjectId) -> bool {
    b.runner.state().objects[&id].has_keyword(&Keyword::Indestructible)
}

#[test]
fn unattaching_taps_that_creature_and_makes_it_indestructible() {
    let mut b = board();
    let offered = activate(&mut b, true, None);
    assert_eq!(
        offered,
        Some(vec![TargetRef::Object(b.blade)]),
        "only the Equipment on a creature P0 controls is offered"
    );
    assert_eq!(
        b.runner.state().objects[&b.blade].attached_to,
        None,
        "the chosen Equipment is unattached (and stays on the battlefield)"
    );
    assert!(tapped(&b, b.bearer), "that creature is tapped");
    assert!(
        indestructible(&b, b.bearer),
        "that creature gains indestructible"
    );
    assert!(
        !indestructible(&b, b.akiri),
        "\"it\" is the creature, not Akiri"
    );
    assert!(
        b.runner.state().objects[&b.rival_blade]
            .attached_to
            .is_some(),
        "the opponent's Equipment is untouched"
    );
}

#[test]
fn declining_does_nothing() {
    let mut b = board();
    activate(&mut b, false, None);
    assert_eq!(
        b.runner.state().objects[&b.blade].attached_to,
        Some(AttachTarget::Object(b.bearer))
    );
    assert!(!tapped(&b, b.bearer));
    assert!(!indestructible(&b, b.bearer));
}

/// With two equipped creatures, the player picks WHICH Equipment comes off, and
/// "that creature" follows the pick — not the first equipped creature found.
#[test]
fn the_chosen_equipment_decides_which_creature_is_tapped() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let akiri = scenario
        .add_creature_from_oracle(P0, "Akiri, Fearless Voyager", 3, 3, AKIRI)
        .id();
    let first = scenario.add_creature(P0, "First", 2, 2).id();
    let first_blade = scenario
        .add_artifact_from_oracle(P0, "First Blade", BLADE)
        .with_subtypes(vec!["Equipment"])
        .id();
    let second = scenario.add_creature(P0, "Second", 2, 2).id();
    let second_blade = scenario
        .add_artifact_from_oracle(P0, "Second Blade", BLADE)
        .with_subtypes(vec!["Equipment"])
        .id();
    scenario.add_basic_land(P0, ManaColor::White);
    let mut runner = scenario.build();
    attach(&mut runner, first_blade, first);
    attach(&mut runner, second_blade, second);
    let mut b = Board {
        runner,
        akiri,
        bearer: first,
        blade: first_blade,
        rival_blade: second_blade,
    };
    let offered = activate(&mut b, true, Some(second_blade)).expect("a choice is raised");
    assert_eq!(offered.len(), 2, "both Equipment are offered");
    assert_eq!(b.runner.state().objects[&second_blade].attached_to, None);
    assert!(tapped(&b, second) && indestructible(&b, second));
    assert!(!tapped(&b, first) && !indestructible(&b, first));
    assert_eq!(
        b.runner.state().objects[&first_blade].attached_to,
        Some(AttachTarget::Object(first))
    );
}
