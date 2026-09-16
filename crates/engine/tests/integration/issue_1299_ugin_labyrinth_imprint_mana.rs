//! Regression for issue #1299 — Ugin's Labyrinth must produce two colorless
//! mana when a card is imprinted (exiled with it).
//!
//! Oracle:
//!   Imprint — When this land enters, you may exile a colorless card with mana
//!   value 7 or greater from your hand.
//!   {T}: Add {C}. If a card is exiled with Ugin's Labyrinth, add {C}{C} instead.
//!
//! CR 605.3b: Mana abilities resolve immediately on activation.
//! CR 614.1a: The "instead" clause is a replacement; the parsed shape is base
//! `{C}` plus a +1 colorless delta when `CardsExiledBySource >= 1`.
//! CR 406.6 + CR 607.2a: Imprint exiles must be linked to the source so the
//! mana ability's condition can read them.

use engine::game::scenario::{GameRunner, GameScenario, P0};
use engine::game::zones::{add_to_zone, create_object, remove_from_zone};
use engine::types::ability::{Effect, QuantityExpr, QuantityRef, TargetRef};
use engine::types::card_type::CoreType;
use engine::types::identifiers::CardId;
use engine::types::mana::{ManaCost, ManaType};
use engine::types::phase::Phase;
use engine::types::zones::Zone;

const UGIN_LABYRINTH_WITH_RETURN: &str = "Imprint — When this land enters, you may exile a colorless card with mana value 7 or greater from your hand.\n\
{T}: Add {C}. If a card is exiled with Ugin's Labyrinth, add {C}{C} instead.\n\
{T}: Return the exiled card to its owner's hand.";

const UGIN_LABYRINTH: &str = "Imprint — When this land enters, you may exile a colorless card with mana value 7 or greater from your hand.\n\
{T}: Add {C}. If a card is exiled with Ugin's Labyrinth, add {C}{C} instead.";

fn add_colorless_imprint_candidate(
    runner: &mut GameRunner,
) -> engine::types::identifiers::ObjectId {
    let id = create_object(
        runner.state_mut(),
        CardId(990_001),
        P0,
        "Colorless Seven-Drop".to_string(),
        Zone::Hand,
    );
    let obj = runner.state_mut().objects.get_mut(&id).unwrap();
    obj.mana_cost = ManaCost::generic(10);
    obj.color.clear();
    obj.card_types.core_types.push(CoreType::Creature);
    id
}

fn resolve_imprint_exile(
    runner: &mut GameRunner,
    labyrinth: engine::types::identifiers::ObjectId,
    imprint_candidate: engine::types::identifiers::ObjectId,
) {
    let trigger = &runner.state().objects[&labyrinth].trigger_definitions[0];
    let execute = trigger
        .definition
        .execute
        .as_ref()
        .expect("imprint trigger must carry an execute ability");
    let mut resolved = engine::game::ability_utils::build_resolved_from_def(execute, labyrinth, P0);
    resolved.targets = vec![TargetRef::Object(imprint_candidate)];
    let mut events = Vec::new();
    engine::game::effects::change_zone::resolve(runner.state_mut(), &resolved, &mut events)
        .expect("imprint exile must resolve");
}

#[test]
fn ugin_labyrinth_with_imprinted_card_produces_two_colorless() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let labyrinth = scenario
        .add_land_to_hand(P0, "Ugin's Labyrinth")
        .from_oracle_text(UGIN_LABYRINTH)
        .id();

    let mut runner = scenario.build();
    remove_from_zone(runner.state_mut(), labyrinth, Zone::Hand, P0);
    add_to_zone(runner.state_mut(), labyrinth, Zone::Battlefield, P0);
    runner.state_mut().objects.get_mut(&labyrinth).unwrap().zone = Zone::Battlefield;
    engine::game::trigger_index::reindex_object_triggers(runner.state_mut(), labyrinth);

    let mana_def = &runner.state().objects[&labyrinth].abilities[0];
    match mana_def.effect.as_ref() {
        Effect::Mana { .. } => {}
        other => panic!("expected mana ability at index 0, got {other:?}"),
    }
    let sub = mana_def
        .sub_ability
        .as_ref()
        .expect("mana ability must carry imprint delta sub_ability");
    match sub.condition.as_ref() {
        Some(engine::types::ability::AbilityCondition::QuantityCheck {
            lhs:
                QuantityExpr::Ref {
                    qty: QuantityRef::CardsExiledBySource,
                },
            ..
        }) => {}
        other => panic!("expected CardsExiledBySource condition, got {other:?}"),
    }

    let imprint_candidate = add_colorless_imprint_candidate(&mut runner);
    resolve_imprint_exile(&mut runner, labyrinth, imprint_candidate);

    assert!(
        runner
            .state()
            .exile_links
            .iter()
            .any(|link| link.source_id == labyrinth && link.exiled_id == imprint_candidate),
        "imprint exile must populate exile_links (CR 406.6 + CR 607.2a)"
    );

    let outcome = runner.activate(labyrinth, 0).resolve();

    assert_eq!(
        outcome.mana_pool_color(P0, ManaType::Colorless),
        2,
        "imprinted Ugin's Labyrinth must produce 2 colorless (1 base + 1 delta)"
    );
    assert_eq!(outcome.mana_pool_total(P0), 2);
}

#[test]
fn ugin_labyrinth_without_imprint_produces_one_colorless() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let labyrinth = scenario
        .add_land_to_hand(P0, "Ugin's Labyrinth")
        .from_oracle_text(UGIN_LABYRINTH)
        .id();

    let mut runner = scenario.build();
    remove_from_zone(runner.state_mut(), labyrinth, Zone::Hand, P0);
    add_to_zone(runner.state_mut(), labyrinth, Zone::Battlefield, P0);
    runner.state_mut().objects.get_mut(&labyrinth).unwrap().zone = Zone::Battlefield;

    let outcome = runner.activate(labyrinth, 0).resolve();

    assert_eq!(
        outcome.mana_pool_color(P0, ManaType::Colorless),
        1,
        "Labyrinth without an imprint must produce only the base colorless mana"
    );
}

/// Field report (2026-09-16): activating "{T}: Return the exiled card to its
/// owner's hand" tapped the land and returned NOTHING — the imprinted card sat
/// in exile for the rest of the game.
///
/// CR 607.2a + CR 608.2c: inside an ACTIVATED ability of the permanent that did
/// the exiling, "the exiled card" is the LINKED card. The imprint happens in a
/// separate enters TRIGGER, so this ability's own chain publishes no tracked
/// set — the bare anaphor bound to an empty `TrackedSet` and moved nothing.
///
/// This exercises the whole path, not just the parse: the filter must resolve
/// through `exile_links` and the return must actually move the card out of
/// exile and into its owner's hand.
#[test]
fn ugin_labyrinth_returns_the_imprinted_card_to_hand() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let labyrinth = scenario
        .add_land_to_hand(P0, "Ugin's Labyrinth")
        .from_oracle_text(UGIN_LABYRINTH_WITH_RETURN)
        .id();

    let mut runner = scenario.build();
    remove_from_zone(runner.state_mut(), labyrinth, Zone::Hand, P0);
    add_to_zone(runner.state_mut(), labyrinth, Zone::Battlefield, P0);
    runner.state_mut().objects.get_mut(&labyrinth).unwrap().zone = Zone::Battlefield;
    engine::game::trigger_index::reindex_object_triggers(runner.state_mut(), labyrinth);

    let imprint_candidate = add_colorless_imprint_candidate(&mut runner);
    resolve_imprint_exile(&mut runner, labyrinth, imprint_candidate);
    assert_eq!(
        runner.state().objects[&imprint_candidate].zone,
        Zone::Exile,
        "the imprint must put the card in exile first"
    );

    let return_index = runner.state().objects[&labyrinth]
        .abilities
        .iter()
        .position(|a| matches!(a.effect.as_ref(), Effect::Bounce { .. }))
        .expect("the land must carry the return ability");

    runner.activate(labyrinth, return_index).resolve();

    assert_eq!(
        runner.state().objects[&imprint_candidate].zone,
        Zone::Hand,
        "the card exiled with this land must come back to its owner's hand"
    );
    assert!(
        runner.state().players[P0.0 as usize]
            .hand
            .contains(&imprint_candidate),
        "and it must be in the owner's hand list, not just carry the zone tag"
    );
}
