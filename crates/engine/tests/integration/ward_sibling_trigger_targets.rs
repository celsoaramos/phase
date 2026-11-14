//! CR 603.2 + CR 603.3b + CR 702.21a: a triggered ability's targets are chosen
//! as it is put on the stack, and a permanent with ward that becomes one of those
//! targets triggers then. When the same event fires two triggered abilities that
//! both need a target, the first one's targets are announced in an action that
//! ends on the SECOND ability's target prompt, not on priority, so the priority
//! pipeline never scans that action's events. Ward must still trigger.
//!
//! Field report (2026-11-02): Sheltered by Ghosts entered alongside an Optimistic
//! Scavenger. Its controller ordered the exile trigger first (target: a creature
//! with ward {1}), then aimed the Scavenger's counter at their own creature. The
//! ward never triggered and the creature was exiled.

use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::types::ability::TargetRef;
use engine::types::actions::GameAction;
use engine::types::game_state::{CastPaymentMode, StackEntryKind, WaitingFor};
use engine::types::identifiers::ObjectId;
use engine::types::mana::{ManaCost, ManaCostShard, ManaType, ManaUnit};
use engine::types::phase::Phase;
use engine::types::zones::Zone;

const WATERFALL_AERIALIST: &str = "Flying\nWard {2} (Whenever this creature becomes the target of a spell or ability an opponent controls, counter it unless that player pays {2}.)";
// The Room half of its Eerie trigger is irrelevant here and left out.
const OPTIMISTIC_SCAVENGER: &str =
    "Whenever an enchantment you control enters, put a +1/+1 counter on target creature.";
const BANISHING_LIGHT: &str = "When this enchantment enters, exile target nonland permanent an opponent controls until this enchantment leaves the battlefield.";

struct Board {
    runner: GameRunner,
    ward: ObjectId,
    scavenger: ObjectId,
    light: ObjectId,
}

fn board() -> Board {
    let mut sc = GameScenario::new();
    sc.at_phase(Phase::PreCombatMain);
    let ward = sc
        .add_creature(P1, "Waterfall Aerialist", 3, 1)
        .from_oracle_text_with_keywords(&["Flying", "Ward"], WATERFALL_AERIALIST)
        .id();
    // A second legal target, so the exile trigger asks for its target instead
    // of taking the only one.
    sc.add_creature(P1, "Grizzly Bears", 2, 2);
    let scavenger = sc
        .add_creature(P0, "Optimistic Scavenger", 1, 1)
        .from_oracle_text(OPTIMISTIC_SCAVENGER)
        .id();
    let light = {
        let mut builder = sc.add_creature_to_hand(P0, "Banishing Light", 0, 0);
        builder
            .as_enchantment()
            .with_mana_cost(ManaCost::Cost {
                shards: vec![ManaCostShard::White],
                generic: 2,
            })
            .from_oracle_text(BANISHING_LIGHT);
        builder.id()
    };
    sc.with_mana_pool(
        P0,
        [ManaType::White; 3]
            .iter()
            .map(|t| ManaUnit::new(*t, ObjectId(0), false, vec![]))
            .collect(),
    );
    Board {
        runner: sc.build(),
        ward,
        scavenger,
        light,
    }
}

fn ward_triggers(runner: &GameRunner, ward: ObjectId) -> usize {
    runner
        .state()
        .stack
        .iter()
        .filter(|entry| {
            matches!(&entry.kind, StackEntryKind::TriggeredAbility { source_id, .. } if *source_id == ward)
        })
        .count()
}

/// Casts Banishing Light, lets it resolve and puts both enters triggers on the
/// stack. `light_first`: the exile trigger's target is announced first (the
/// field report's order); otherwise the Scavenger's is.
fn put_both_triggers(b: &mut Board, light_first: bool) {
    let card_id = b.runner.state().objects[&b.light].card_id;
    b.runner
        .act(GameAction::CastSpell {
            object_id: b.light,
            card_id,
            targets: vec![],
            payment_mode: CastPaymentMode::Auto,
        })
        .expect("Banishing Light is cast");
    let mut ordered = false;
    let mut aimed = Vec::new();
    for _ in 0..32 {
        let action = match b.runner.state().waiting_for.clone() {
            WaitingFor::Priority { .. } if aimed.len() == 2 => break,
            WaitingFor::Priority { .. } => GameAction::PassPriority,
            WaitingFor::OrderTriggers { triggers, .. } => {
                ordered = true;
                let light = triggers
                    .iter()
                    .position(|t| t.source_id == b.light)
                    .expect("the exile trigger is ordered");
                let other = 1 - light;
                let order = if light_first {
                    vec![light, other]
                } else {
                    vec![other, light]
                };
                GameAction::OrderTriggers { order }
            }
            WaitingFor::TriggerTargetSelection { source_id, .. } => {
                let source_id = source_id.expect("the trigger has a source");
                aimed.push(source_id);
                let target = if source_id == b.light {
                    b.ward
                } else {
                    b.scavenger
                };
                GameAction::SelectTargets {
                    targets: vec![TargetRef::Object(target)],
                }
            }
            other => panic!("unexpected prompt after aiming {aimed:?}: {other:?}"),
        };
        b.runner.act(action).expect("the prompt accepts its answer");
    }
    assert!(ordered, "reach guard: both triggers were ordered");
    let expected = if light_first {
        [b.light, b.scavenger]
    } else {
        [b.scavenger, b.light]
    };
    assert_eq!(
        aimed, expected,
        "reach guard: targets announced in this order"
    );
}

fn resolve_all(runner: &mut GameRunner) {
    for _ in 0..32 {
        if runner.state().stack.is_empty()
            && matches!(runner.state().waiting_for, WaitingFor::Priority { .. })
        {
            return;
        }
        let action = match runner.state().waiting_for.clone() {
            WaitingFor::Priority { .. } => GameAction::PassPriority,
            WaitingFor::UnlessPayment { .. } => GameAction::PayUnlessCost { pay: false },
            other => panic!("unexpected prompt while resolving: {other:?}"),
        };
        runner.act(action).expect("resolution prompt accepts");
    }
    panic!("the stack did not resolve");
}

/// The reported order: ward triggers although the action that announced the
/// exile trigger's target ended on the Scavenger's target prompt.
#[test]
fn ward_triggers_when_a_sibling_trigger_still_needs_a_target() {
    let mut b = board();
    put_both_triggers(&mut b, true);
    assert_eq!(ward_triggers(&b.runner, b.ward), 1);
    // CR 603.3b: it waited for the batch and went on the stack above both.
    assert!(
        matches!(
            &b.runner.state().stack.last().expect("the stack holds entries").kind,
            StackEntryKind::TriggeredAbility { source_id, .. } if *source_id == b.ward
        ),
        "ward is the top of the stack"
    );

    resolve_all(&mut b.runner);
    assert_eq!(
        b.runner.state().objects[&b.ward].zone,
        Zone::Battlefield,
        "ward countered the unpaid exile trigger"
    );
}

/// Control: the exile trigger announced last already worked; it still triggers
/// ward exactly once.
#[test]
fn ward_triggers_once_when_its_target_is_announced_last() {
    let mut b = board();
    put_both_triggers(&mut b, false);
    assert_eq!(ward_triggers(&b.runner, b.ward), 1);

    resolve_all(&mut b.runner);
    assert_eq!(b.runner.state().objects[&b.ward].zone, Zone::Battlefield);
}
