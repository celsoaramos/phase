//! "If you revealed a Dragon card OR controlled a Dragon as you cast this spell."
//!
//! The Dragons of Tarkir rider (Draconic Roar, Foul-Tongue Invocation, Orator of
//! Ojutai, Silumgar's Scorn, …) gates a bonus on either of two alternatives: the
//! spell's optional "reveal a Dragon card" additional cost was paid, OR the
//! caster already controlled a Dragon when the spell was cast. A player holding
//! the board reveals nothing, so it is an `Or`, never an `And`.
//!
//! Before the condition was recognized the whole gate was DROPPED, and the bonus
//! fired for everyone: Draconic Roar burned the creature's controller for 3 with
//! no Dragon anywhere. Coverage called the card supported the whole time — parse
//! is not execution — so the miss reached the table.
//!
//! These tests drive the real cast pipeline three times, one per branch of the
//! truth table, and the NEGATIVE case is the one that pins the fix: no Dragon
//! revealed and none controlled must leave the opponent at 20. The targeted
//! creature has toughness 4 so it survives the 3 damage in every branch and the
//! opponent's life is the only thing that moves.

use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::types::ability::TargetRef;
use engine::types::actions::GameAction;
use engine::types::game_state::{CastPaymentMode, WaitingFor};
use engine::types::identifiers::ObjectId;
use engine::types::phase::Phase;

const DRACONIC_ROAR_ORACLE: &str = "As an additional cost to cast this spell, you may reveal a \
Dragon card from your hand.\nDraconic Roar deals 3 damage to target creature. If you revealed a \
Dragon card or controlled a Dragon as you cast this spell, Draconic Roar deals 3 damage to that \
creature's controller.";

/// How the caster satisfies (or fails to satisfy) the rider.
enum Dragon {
    /// No Dragon on the battlefield and none revealed — the rider must NOT fire.
    None,
    /// A Dragon on the battlefield; the optional cost is declined anyway.
    Controlled,
    /// No Dragon on the battlefield; one revealed from hand to pay the cost.
    Revealed,
}

/// Cast Draconic Roar at an opponent's 2/4 and resolve it. Returns the runner
/// and the opponent's life afterwards.
fn cast_draconic_roar(dragon: Dragon) -> (GameRunner, i32) {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);

    let mut revealable: Option<ObjectId> = None;
    match dragon {
        Dragon::None => {}
        Dragon::Controlled => {
            scenario
                .add_creature(P0, "Board Dragon", 4, 4)
                .with_subtypes(vec!["Dragon"]);
        }
        Dragon::Revealed => {
            revealable = Some(
                scenario
                    .add_creature_to_hand(P0, "Hand Dragon", 4, 4)
                    .with_subtypes(vec!["Dragon"])
                    .id(),
            );
        }
    }
    // Toughness 4 survives the 3 damage in every branch, so the opponent's life
    // is the only reading that separates "rider fired" from "rider did not".
    let target = scenario.add_creature(P1, "Tough Bear", 2, 4).id();

    let mut builder =
        scenario.add_spell_to_hand_from_oracle(P0, "Draconic Roar", true, DRACONIC_ROAR_ORACLE);
    builder.with_mana_cost(engine::types::mana::ManaCost::generic(0));
    let spell = builder.id();

    let mut runner = scenario.build();
    let card_id = runner.state().objects[&spell].card_id;
    runner
        .act(GameAction::CastSpell {
            object_id: spell,
            card_id,
            targets: vec![],
            payment_mode: CastPaymentMode::Auto,
        })
        .expect("casting Draconic Roar must be accepted");

    let pay = matches!(dragon, Dragon::Revealed);
    for _ in 0..32 {
        match runner.state().waiting_for.clone() {
            WaitingFor::OptionalCostChoice { .. } => {
                runner
                    .act(GameAction::DecideOptionalCost { pay })
                    .expect("deciding the optional reveal must succeed");
            }
            WaitingFor::PayCost { choices, .. } => {
                let card = revealable.expect("only the reveal branch reaches a cost prompt");
                assert!(
                    choices.contains(&card),
                    "the Dragon in hand must be revealable: {choices:?}"
                );
                runner
                    .act(GameAction::SelectCards { cards: vec![card] })
                    .expect("revealing the Dragon must succeed");
            }
            WaitingFor::TargetSelection { target_slots, .. } => {
                assert!(
                    target_slots[0]
                        .legal_targets
                        .contains(&TargetRef::Object(target)),
                    "the opponent creature must be a legal damage target"
                );
                runner
                    .act(GameAction::SelectTargets {
                        targets: vec![TargetRef::Object(target)],
                    })
                    .expect("targeting the opponent creature must succeed");
            }
            WaitingFor::Priority { .. } => {
                if runner.state().stack.is_empty() {
                    break;
                }
                if runner.act(GameAction::PassPriority).is_err() {
                    break;
                }
            }
            other => panic!("unexpected prompt while casting Draconic Roar: {other:?}"),
        }
    }

    assert_eq!(
        runner.state().objects[&target].damage_marked,
        3,
        "the targeted creature always takes the printed 3"
    );
    let life = runner.state().players[P1.0 as usize].life;
    (runner, life)
}

/// No Dragon revealed and none controlled: the rider must NOT fire. This is the
/// case the dropped condition got wrong — the opponent was burned for 3 with no
/// Dragon anywhere.
#[test]
fn draconic_roar_without_a_dragon_spares_the_controller() {
    let (_runner, life) = cast_draconic_roar(Dragon::None);

    assert_eq!(
        life, 20,
        "with no Dragon revealed and none controlled, Draconic Roar must not touch the \
         creature's controller"
    );
}

/// The board leg: a Dragon already on the battlefield satisfies the rider with
/// the optional cost declined.
#[test]
fn draconic_roar_with_a_controlled_dragon_burns_the_controller() {
    let (_runner, life) = cast_draconic_roar(Dragon::Controlled);

    assert_eq!(
        life, 17,
        "controlling a Dragon as the spell was cast must fire the rider for 3"
    );
}

/// The cost leg: revealing a Dragon from hand satisfies the rider with nothing
/// on the battlefield.
#[test]
fn draconic_roar_with_a_revealed_dragon_burns_the_controller() {
    let (_runner, life) = cast_draconic_roar(Dragon::Revealed);

    assert_eq!(
        life, 17,
        "revealing a Dragon card as the spell was cast must fire the rider for 3"
    );
}
