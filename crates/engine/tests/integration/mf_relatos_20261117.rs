//! Relatos da mesa de 17/11/2026 (motor v0.125.0).
//!
//! - **Deep Space 9**: "When Deep Space 9 enters and at the beginning of your
//!   upkeep, exile the top four cards of your library. Put all permanent cards
//!   exiled this way onto the battlefield." — exilava as quatro e nenhuma
//!   permanente chegava ao campo.
//! - **Staff of Eden, Vault's Key**: "put target legendary permanent card not
//!   named Staff of Eden, Vault's Key from a graveyard onto the battlefield under
//!   your control." — o alvo perdia "from a graveyard" (e o "not named"), e o
//!   motor oferecia as permanentes lendárias do CAMPO.
//! - **Elbrus, the Binding Blade**: "When equipped creature deals combat damage
//!   to a player, unattach Elbrus, then transform it." — "unattach ~" era lacuna
//!   e o "it" virava a criatura equipada: a Elbrus não se transformava.

use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::parser::oracle::parse_oracle_text;
use engine::types::ability::{Effect, FilterProp, TargetFilter};
use engine::types::actions::GameAction;
use engine::types::game_state::WaitingFor;
use engine::types::identifiers::ObjectId;
use engine::types::mana::ManaCost;
use engine::types::phase::Phase;
use engine::types::zones::Zone;

const DEEP_SPACE_9: &str = "When Deep Space 9 enters and at the beginning of your upkeep, exile \
the top four cards of your library. Put all permanent cards exiled this way onto the battlefield.";

const STAFF_OF_EDEN: &str = "When Staff of Eden enters, put target legendary permanent card not \
named Staff of Eden, Vault's Key from a graveyard onto the battlefield under your control.\n\
{T}: Draw a card for each permanent you control but don't own.";

const ELBRUS: &str = "Equipped creature gets +1/+0.\nWhen equipped creature deals combat damage \
to a player, unattach Elbrus, then transform it.\nEquip {1}";

fn settle(runner: &mut GameRunner) {
    for _ in 0..80 {
        match runner.state().waiting_for.clone() {
            WaitingFor::ManaPayment { .. } => {
                runner.act(GameAction::PassPriority).expect("pay");
            }
            WaitingFor::Priority { .. } => {
                if runner.state().stack.is_empty() {
                    return;
                }
                runner.act(GameAction::PassPriority).expect("pass");
            }
            other => panic!("unexpected prompt: {other:?}"),
        }
    }
    panic!("did not settle");
}

fn zone_of(runner: &GameRunner, id: ObjectId) -> Zone {
    runner.state().objects[&id].zone
}

#[test]
fn deep_space_9_puts_the_permanent_cards_it_exiled_onto_the_battlefield() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    // Library top first: a land, an instant, a land, an instant (all four exiled).
    let instant_b = scenario.add_spell_to_library_top(P0, "Opt", true).id();
    let land_b = scenario.add_land_to_library_top(P0, "Island").id();
    let instant_a = scenario.add_spell_to_library_top(P0, "Shock", true).id();
    let land_a = scenario.add_land_to_library_top(P0, "Forest").id();
    let station = {
        let mut card = scenario.add_artifact_to_hand_from_oracle(P0, "Deep Space 9", DEEP_SPACE_9);
        card.with_mana_cost(ManaCost::Cost {
            shards: vec![],
            generic: 0,
        });
        card.id()
    };
    let mut runner = scenario.build();
    let card_id = runner.state().objects[&station].card_id;
    runner
        .act(GameAction::CastSpell {
            object_id: station,
            card_id,
            targets: vec![],
            payment_mode: engine::types::game_state::CastPaymentMode::Auto,
        })
        .expect("cast Deep Space 9");
    settle(&mut runner);

    assert_eq!(zone_of(&runner, station), Zone::Battlefield);
    assert_eq!(
        zone_of(&runner, land_a),
        Zone::Battlefield,
        "the exiled land is a permanent card: it goes onto the battlefield"
    );
    assert_eq!(zone_of(&runner, land_b), Zone::Battlefield);
    assert_eq!(
        zone_of(&runner, instant_a),
        Zone::Exile,
        "an instant is not a permanent card: it stays exiled"
    );
    assert_eq!(zone_of(&runner, instant_b), Zone::Exile);
}

fn first_trigger_effect(oracle: &str, name: &str, types: &[&str]) -> Effect {
    let types: Vec<String> = types.iter().map(|t| t.to_string()).collect();
    let parsed = parse_oracle_text(oracle, name, &[], &types, &[]);
    let trigger = parsed.triggers.first().expect("a trigger");
    (*trigger.execute.as_ref().expect("execute").effect).clone()
}

#[test]
fn staff_of_eden_targets_a_legendary_permanent_card_in_a_graveyard_not_named_itself() {
    let effect = first_trigger_effect(
        STAFF_OF_EDEN,
        "Staff of Eden, Vault's Key",
        &["Legendary", "Artifact"],
    );
    let Effect::ChangeZone { target, .. } = &effect else {
        panic!("expected ChangeZone, got {effect:?}");
    };
    let TargetFilter::Typed(typed) = target else {
        panic!("expected a typed target, got {target:?}");
    };
    assert!(
        typed
            .properties
            .iter()
            .any(|p| matches!(p, FilterProp::InZone { zone } if *zone == Zone::Graveyard)),
        "\"from a graveyard\" is the target's zone: {typed:?}"
    );
    assert!(
        typed.properties.iter().any(|p| matches!(
            p,
            FilterProp::Not { prop } if matches!(&**prop, FilterProp::Named { name }
                if name.eq_ignore_ascii_case("Staff of Eden, Vault's Key"))
        )),
        "\"not named Staff of Eden, Vault's Key\" excludes that name: {typed:?}"
    );
}

#[test]
fn staff_of_eden_returns_the_legendary_card_from_the_graveyard_not_a_permanent_in_play() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let in_graveyard = {
        let mut card = scenario.add_creature_to_graveyard(P1, "Bayek of Siwa", 4, 4);
        card.as_legendary();
        card.id()
    };
    let in_play = {
        let mut card = scenario.add_creature(P1, "Hermes, Overseer of Elpis", 2, 4);
        card.as_legendary();
        card.id()
    };
    let staff = {
        let mut card = scenario.add_artifact_to_hand_from_oracle(
            P0,
            "Staff of Eden, Vault's Key",
            STAFF_OF_EDEN,
        );
        card.as_legendary();
        card.with_mana_cost(ManaCost::Cost {
            shards: vec![],
            generic: 0,
        });
        card.id()
    };
    let mut runner = scenario.build();
    let card_id = runner.state().objects[&staff].card_id;
    runner
        .act(GameAction::CastSpell {
            object_id: staff,
            card_id,
            targets: vec![],
            payment_mode: engine::types::game_state::CastPaymentMode::Auto,
        })
        .expect("cast Staff of Eden");
    for _ in 0..40 {
        match runner.state().waiting_for.clone() {
            WaitingFor::TriggerTargetSelection { .. } => {
                let legal = engine::ai_support::legal_actions(runner.state());
                let offers_in_play = legal
                    .iter()
                    .any(|a| format!("{a:?}").contains(&format!("ObjectId({})", in_play.0)));
                assert!(
                    !offers_in_play,
                    "a permanent on the battlefield is not a card in a graveyard: {legal:?}"
                );
                let pick = legal
                    .iter()
                    .find(|a| format!("{a:?}").contains(&format!("ObjectId({})", in_graveyard.0)))
                    .cloned()
                    .unwrap_or_else(|| panic!("the graveyard card is a legal target: {legal:?}"));
                runner.act(pick).expect("target the legendary card");
            }
            WaitingFor::ManaPayment { .. } => {
                runner.act(GameAction::PassPriority).expect("pay");
            }
            WaitingFor::Priority { .. } => {
                if runner.state().stack.is_empty() {
                    break;
                }
                runner.act(GameAction::PassPriority).expect("pass");
            }
            other => panic!("unexpected prompt: {other:?}"),
        }
    }
    assert_eq!(zone_of(&runner, in_graveyard), Zone::Battlefield);
    assert_eq!(runner.state().objects[&in_graveyard].controller, P0);
    assert_eq!(zone_of(&runner, in_play), Zone::Battlefield);
    assert_eq!(runner.state().objects[&in_play].controller, P1);
}

#[test]
fn elbrus_unattaches_itself_then_transforms_itself() {
    let effect = first_trigger_effect(
        ELBRUS,
        "Elbrus, the Binding Blade",
        &["Legendary", "Artifact"],
    );
    let Effect::UnattachAll { attachment, .. } = &effect else {
        panic!("\"unattach ~\" is no longer a gap: {effect:?}");
    };
    assert_eq!(*attachment, TargetFilter::SelfRef);
    let types = vec!["Legendary".to_string(), "Artifact".to_string()];
    let parsed = parse_oracle_text(ELBRUS, "Elbrus, the Binding Blade", &[], &types, &[]);
    let sub = parsed.triggers[0]
        .execute
        .as_ref()
        .unwrap()
        .sub_ability
        .as_ref()
        .expect("then transform it");
    assert!(
        matches!(
            &*sub.effect,
            Effect::Transform {
                target: TargetFilter::SelfRef,
                ..
            }
        ),
        "\"it\" is Elbrus, not the equipped creature: {:?}",
        sub.effect
    );
}
