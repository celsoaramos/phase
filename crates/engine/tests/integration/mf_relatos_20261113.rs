//! Relatos da mesa (13/11/2026, rodada das 11h).
//!
//! - Janice Rand, Essential Aide — "Whenever a commander you control enters or
//!   attacks, put a +1/+1 counter on it, then reveal …". With the commander
//!   attacking alongside other creatures, the `EntersOrAttacks` trigger fired on
//!   the whole declaration, so "it" (the triggering attacker) bound nothing and
//!   the counter was lost; the reveal still happened.
//! - U.S.S. Defiant, Mirror Migrant — "When U.S.S. Defiant enters, it and up to
//!   one target creature you control phase out." The compound subject
//!   ("it and up to one target …") was an `unbound_subject` gap.
//! - Iron-Shield Elf — "Discard a card: Iron-Shield Elf gains indestructible
//!   until end of turn. Tap it." The "Tap it" rider bound to an absent parent
//!   target, so the Elf stayed untapped and could attack.

use engine::game::combat::AttackTarget;
use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::types::ability::TargetRef;
use engine::types::actions::GameAction;
use engine::types::counter::CounterType;
use engine::types::game_state::{CastPaymentMode, WaitingFor};
use engine::types::identifiers::ObjectId;
use engine::types::phase::Phase;
use engine::types::CoreType;

const JANICE: &str = "Whenever a commander you control enters or attacks, put a +1/+1 counter on it, then reveal cards from the top of your library until you reveal a land card. Put that card onto the battlefield tapped and the rest on the bottom of your library in a random order.";

const DEFIANT: &str = "Flash\nWhen U.S.S. Defiant enters, it and up to one target creature you control phase out. (Treat them and anything attached to them as though they don't exist until your next turn.)";

const IRON_SHIELD_ELF: &str =
    "Discard a card: Iron-Shield Elf gains indestructible until end of turn. Tap it.";

/// Resolve the stack, answering target prompts with `target` and accepting
/// every optional choice.
fn drain(runner: &mut GameRunner, target: Option<ObjectId>) {
    for _ in 0..80 {
        let action = match runner.state().waiting_for.clone() {
            WaitingFor::TargetSelection { .. } | WaitingFor::TriggerTargetSelection { .. } => {
                GameAction::ChooseTarget {
                    target: target.map(TargetRef::Object),
                }
            }
            WaitingFor::OptionalEffectChoice { .. } => {
                GameAction::DecideOptionalEffect { accept: true }
            }
            WaitingFor::Priority { .. } if !runner.state().stack.is_empty() => {
                GameAction::PassPriority
            }
            _ => break,
        };
        if runner.act(action).is_err() {
            break;
        }
    }
}

fn plus_counters(runner: &GameRunner, id: ObjectId) -> u32 {
    runner.state().objects[&id]
        .counters
        .get(&CounterType::Plus1Plus1)
        .copied()
        .unwrap_or(0)
}

fn janice_scenario() -> (GameRunner, ObjectId, ObjectId) {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.add_creature_from_oracle(P0, "Janice Rand, Essential Aide", 2, 3, JANICE);
    let commander = scenario
        .add_creature(P0, "Commander Spock", 3, 3)
        .commander()
        .id();
    let other = scenario.add_creature(P0, "Sun Titan", 6, 6).id();
    let land = scenario.add_card_to_library_top(P0, "Fabled Passage");
    let mut runner = scenario.build();
    {
        let obj = runner.state_mut().objects.get_mut(&land).unwrap();
        obj.card_types.core_types.push(CoreType::Land);
        obj.base_card_types = obj.card_types.clone();
    }
    (runner, commander, other)
}

#[test]
fn janice_counter_lands_on_commander_attacking_with_others() {
    let (mut runner, commander, other) = janice_scenario();
    runner.advance_to_combat();
    runner
        .declare_attackers(&[
            (commander, AttackTarget::Player(P1)),
            (other, AttackTarget::Player(P1)),
        ])
        .expect("both attack");
    drain(&mut runner, None);
    assert_eq!(
        plus_counters(&runner, commander),
        1,
        "the commander that attacked gets the +1/+1 counter even when it attacks with others"
    );
    assert_eq!(
        plus_counters(&runner, other),
        0,
        "the non-commander gets nothing"
    );
}

#[test]
fn janice_counter_lands_on_commander_attacking_alone() {
    let (mut runner, commander, _other) = janice_scenario();
    runner.advance_to_combat();
    runner
        .declare_attackers(&[(commander, AttackTarget::Player(P1))])
        .expect("commander attacks");
    drain(&mut runner, None);
    assert_eq!(plus_counters(&runner, commander), 1);
}

#[test]
fn uss_defiant_phases_out_itself_and_the_target() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let partner = scenario.add_creature(P0, "Christine Chapel", 2, 2).id();
    let defiant = scenario
        .add_creature_to_hand_from_oracle(P0, "U.S.S. Defiant, Mirror Migrant", 4, 4, DEFIANT)
        .id();
    let mut runner = scenario.build();
    let card_id = runner.state().objects[&defiant].card_id;
    runner
        .act(GameAction::CastSpell {
            object_id: defiant,
            card_id,
            targets: vec![],
            payment_mode: CastPaymentMode::Auto,
        })
        .expect("Defiant cast");
    drain(&mut runner, Some(partner));
    assert!(
        runner.state().objects[&defiant]
            .phase_status
            .is_phased_out(),
        "Defiant ('it') phases out"
    );
    assert!(
        runner.state().objects[&partner]
            .phase_status
            .is_phased_out(),
        "the chosen creature phases out"
    );
}

#[test]
fn uss_defiant_phases_out_alone_with_no_target() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let partner = scenario.add_creature(P0, "Christine Chapel", 2, 2).id();
    let defiant = scenario
        .add_creature_to_hand_from_oracle(P0, "U.S.S. Defiant, Mirror Migrant", 4, 4, DEFIANT)
        .id();
    let mut runner = scenario.build();
    let card_id = runner.state().objects[&defiant].card_id;
    runner
        .act(GameAction::CastSpell {
            object_id: defiant,
            card_id,
            targets: vec![],
            payment_mode: CastPaymentMode::Auto,
        })
        .expect("Defiant cast");
    for _ in 0..20 {
        let action = match runner.state().waiting_for.clone() {
            WaitingFor::TargetSelection { .. } | WaitingFor::TriggerTargetSelection { .. } => {
                GameAction::ChooseTarget { target: None }
            }
            WaitingFor::Priority { .. } if !runner.state().stack.is_empty() => {
                GameAction::PassPriority
            }
            _ => break,
        };
        if runner.act(action).is_err() {
            break;
        }
    }
    assert!(runner.state().objects[&defiant]
        .phase_status
        .is_phased_out());
    assert!(!runner.state().objects[&partner]
        .phase_status
        .is_phased_out());
}

#[test]
fn iron_shield_elf_taps_itself() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let elf = scenario
        .add_creature_from_oracle(P0, "Iron-Shield Elf", 1, 1, IRON_SHIELD_ELF)
        .id();
    scenario.add_card_to_hand(P0, "Spare Card");
    let mut runner = scenario.build();
    let mut result = runner
        .act(GameAction::ActivateAbility {
            source_id: elf,
            ability_index: 0,
            payment_mode: CastPaymentMode::Auto,
        })
        .expect("activation accepted");
    for _ in 0..10 {
        match &result.waiting_for {
            WaitingFor::DiscardChoice { cards, .. }
            | WaitingFor::PayCost { choices: cards, .. } => {
                let pick = vec![cards[0]];
                result = runner
                    .act(GameAction::SelectCards { cards: pick })
                    .expect("discard chosen");
            }
            other => {
                eprintln!("iron-shield elf activation waits on {other:?}");
                break;
            }
        }
    }
    drain(&mut runner, None);
    let obj = &runner.state().objects[&elf];
    assert!(obj.tapped, "'Tap it' taps the Elf");
    assert!(
        obj.has_keyword(&engine::types::keywords::Keyword::Indestructible),
        "and it is indestructible"
    );
}
