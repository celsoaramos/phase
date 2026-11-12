//! Relatos da mesa (12/11/2026, rodada das 11h).
//!
//! Archghoul of Thraben — "Whenever this creature or another Zombie you control
//! dies, look at the top card of your library. If it's a Zombie card, you may
//! reveal it and put it into your hand. If you don't put the card into your
//! hand, you may put it into your graveyard." The bare-subtype gate ("a Zombie
//! card", no core type word) fell out of the revealed-card family and became a
//! target-membership check with no target, so the Zombie on top never reached
//! the hand. Same class: any "If it's a <subtype> card" look/reveal gate.

use engine::game::scenario::{GameRunner, GameScenario, P0};
use engine::types::ability::TargetRef;
use engine::types::actions::GameAction;
use engine::types::game_state::{CastPaymentMode, WaitingFor};
use engine::types::identifiers::ObjectId;
use engine::types::phase::Phase;
use engine::types::zones::Zone;
use engine::types::CoreType;

const ARCHGHOUL: &str = "Whenever this creature or another Zombie you control dies, look at the top card of your library. If it's a Zombie card, you may reveal it and put it into your hand. If you don't put the card into your hand, you may put it into your graveyard.";

/// Kill `victim` with a destroy spell and accept every optional choice.
fn kill_and_accept(runner: &mut GameRunner, spell: ObjectId, victim: ObjectId) {
    let cast = GameAction::CastSpell {
        object_id: spell,
        card_id: runner.state().objects[&spell].card_id,
        targets: vec![],
        payment_mode: CastPaymentMode::Auto,
    };
    runner.act(cast).expect("cast accepted");
    for _ in 0..60 {
        let action = match runner.state().waiting_for.clone() {
            WaitingFor::TargetSelection { .. } | WaitingFor::TriggerTargetSelection { .. } => {
                GameAction::ChooseTarget {
                    target: Some(TargetRef::Object(victim)),
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

fn setup(top_is_zombie: bool, victim_is_token: bool) -> (GameRunner, ObjectId) {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario
        .add_creature_from_oracle(P0, "Archghoul of Thraben", 3, 2, ARCHGHOUL)
        .with_subtypes(vec!["Zombie", "Cleric"]);
    let zombie = scenario
        .add_creature(P0, "Zombie", 2, 2)
        .with_subtypes(vec!["Zombie"])
        .id();
    let top = scenario.add_card_to_library_top(P0, "Library Top");
    let spell = scenario
        .add_spell_to_hand_from_oracle(P0, "Murder", true, "Destroy target creature.")
        .id();
    let mut runner = scenario.build();
    // Relato: the Zombies that died were TOKENS (Ghoulcaller Gisa / Victimize
    // sacrifice), which cease to exist once they leave the battlefield — so the gate
    // can't lean on the dead creature still being a Zombie.
    runner
        .state_mut()
        .objects
        .get_mut(&zombie)
        .unwrap()
        .is_token = victim_is_token;
    {
        let obj = runner.state_mut().objects.get_mut(&top).unwrap();
        obj.card_types.core_types.push(CoreType::Creature);
        if top_is_zombie {
            obj.card_types.subtypes.push("Zombie".to_string());
        }
        obj.base_card_types = obj.card_types.clone();
    }
    kill_and_accept(&mut runner, spell, zombie);
    (runner, top)
}

#[test]
fn archghoul_puts_zombie_on_top_into_hand() {
    let (runner, top) = setup(true, true);
    assert_eq!(
        runner.state().objects[&top].zone,
        Zone::Hand,
        "the Zombie card on top goes to hand (wf {:?})",
        runner.state().waiting_for
    );
}

#[test]
fn archghoul_non_zombie_on_top_does_not_go_to_hand() {
    let (runner, top) = setup(false, true);
    assert_ne!(
        runner.state().objects[&top].zone,
        Zone::Hand,
        "a non-Zombie card is never put into hand"
    );
}

/// The gate reads the LOOKED-AT card, not the creature that died: a Zombie card
/// in the graveyard must not let a non-Zombie top card into the hand.
#[test]
fn archghoul_gate_reads_the_top_card_not_the_dead_zombie() {
    let (runner, top) = setup(false, false);
    assert_ne!(runner.state().objects[&top].zone, Zone::Hand);
}

// ---------------------------------------------------------------------------
// Tenza, Godo's Maul (relato 12/11/2026): "Equipped creature gets +1/+1. As long
// as it's legendary, it gets an additional +2/+2. As long as it's red, it has
// trample." The "As long as it's …, it …" sentences bound to the Equipment
// itself, so a legendary red creature only got +1/+1 and no trample. CR 611.3a:
// the pronoun "it" continues the equipped-creature subject of the first sentence.

use engine::game::game_object::AttachTarget;
use engine::types::keywords::Keyword;
use engine::types::mana::ManaColor;
use engine::types::Supertype;

const TENZA: &str = "Equipped creature gets +1/+1. As long as it's legendary, it gets an additional +2/+2. As long as it's red, it has trample.\nEquip {1}";

fn equip_tenza(legendary_red: bool) -> engine::game::game_object::GameObject {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let bearer = {
        let mut b = scenario.add_creature(P0, "Bearer", 3, 3);
        if legendary_red {
            b.as_legendary().with_color(vec![ManaColor::Red]);
        }
        b.id()
    };
    let tenza = scenario
        .add_artifact_from_oracle(P0, "Tenza, Godo's Maul", TENZA)
        .with_subtypes(vec!["Equipment"])
        .id();
    let mut runner = scenario.build();
    {
        let state = runner.state_mut();
        state.objects.get_mut(&tenza).unwrap().attached_to = Some(AttachTarget::Object(bearer));
        state
            .objects
            .get_mut(&bearer)
            .unwrap()
            .attachments
            .push(tenza);
    }
    engine::game::layers::evaluate_layers(runner.state_mut());
    runner.state().objects[&bearer].clone()
}

#[test]
fn tenza_legendary_red_creature_gets_plus_three_and_trample() {
    let bearer = equip_tenza(true);
    assert!(bearer.card_types.supertypes.contains(&Supertype::Legendary));
    assert_eq!(bearer.power, Some(6), "+1/+1 and the additional +2/+2");
    assert_eq!(bearer.toughness, Some(6));
    assert!(bearer.keywords.contains(&Keyword::Trample), "red: trample");
}

#[test]
fn tenza_plain_creature_gets_only_plus_one() {
    let bearer = equip_tenza(false);
    assert_eq!(bearer.power, Some(4));
    assert_eq!(bearer.toughness, Some(4));
    assert!(!bearer.keywords.contains(&Keyword::Trample));
}
