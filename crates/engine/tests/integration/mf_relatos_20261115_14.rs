//! Relato da mesa de 15/11/2026 — **Q, Stacking the Deck**: "When Q enters,
//! shuffle your library, then exile cards from the top of your library until you
//! exile a nonland card. That nonland card gains \"Exile a card from your hand:
//! Create a copy of this card. You may cast the copy. Activate only during your
//! turn and only if this card is exiled.\""
//!
//! O jogador tinha a carta não-terreno exilada pelo Q e a mão cheia, e não
//! conseguia usar a habilidade: "Create a copy of this card" ficava
//! `Unimplemented` (`unparsed_verb_arguments`), então a carta exilada nunca
//! oferecia a ativação.

use engine::game::scenario::{GameRunner, GameScenario, P0};
use engine::parser::oracle::parse_oracle_text;
use engine::types::ability::{CastFromZoneDriver, Duration, Effect, TargetFilter};
use engine::types::actions::{CastChoice, GameAction};
use engine::types::game_state::{CastOfferKind, CastPaymentMode, WaitingFor};
use engine::types::identifiers::ObjectId;
use engine::types::mana::{ManaCost, ManaCostShard, ManaType, ManaUnit};
use engine::types::phase::Phase;
use engine::types::zones::Zone;

const Q: &str = "When Q enters, shuffle your library, then exile cards from the top of your \
library until you exile a nonland card. That nonland card gains \"Exile a card from your hand: \
Create a copy of this card. You may cast the copy. Activate only during your turn and only if \
this card is exiled.\"";

const GRANTED: &str = "Exile a card from your hand: Create a copy of this card. You may cast \
the copy. Activate only during your turn and only if this card is exiled.";

#[test]
fn create_a_copy_of_this_card_parses_and_binds_the_cast_to_the_copy() {
    let parsed = parse_oracle_text(GRANTED, "Some Card", &[], &["Instant".to_string()], &[]);
    let ability = parsed.abilities.first().expect("one activated ability");
    assert!(
        matches!(&*ability.effect, Effect::CreateCardCopyByName { .. }),
        "the copy clause: {:?}",
        ability.effect
    );
    let cast = ability.sub_ability.as_deref().expect("the cast tail");
    let Effect::CastFromZone { target, driver, .. } = &*cast.effect else {
        panic!("expected CastFromZone, got {:?}", cast.effect);
    };
    assert_eq!(*target, TargetFilter::LastCreated);
    assert_eq!(*driver, CastFromZoneDriver::DuringResolution);
}

/// Q in hand with {2}{R}{R} floating; library = Bolt under two lands; one spare
/// card in hand to pay the granted ability with.
fn q_scenario() -> (GameRunner, ObjectId, ObjectId, ObjectId) {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.with_mana_pool(
        P0,
        (0..5)
            .map(|_| ManaUnit::new(ManaType::Red, ObjectId(0), false, vec![]))
            .collect(),
    );
    let bolt = {
        let mut card = scenario.add_spell_to_library_top(P0, "Lightning Bolt", true);
        card.from_oracle_text("Lightning Bolt deals 3 damage to any target.");
        card.with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::Red],
            generic: 0,
        });
        card.id()
    };
    scenario.add_land_to_library_top(P0, "Mountain");
    scenario.add_land_to_library_top(P0, "Mountain");
    let fodder = scenario.add_land_to_hand(P0, "Mountain").id();
    let q = {
        let mut card =
            scenario.add_creature_to_hand_from_oracle(P0, "Q, Stacking the Deck", 4, 4, Q);
        card.with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::Red, ManaCostShard::Red],
            generic: 2,
        });
        card.id()
    };
    (scenario.build(), q, bolt, fodder)
}

fn settle(runner: &mut GameRunner) {
    for _ in 0..12 {
        match runner.state().waiting_for {
            WaitingFor::ManaPayment { .. } => {
                runner.act(GameAction::PassPriority).expect("pay from pool");
            }
            _ => break,
        }
    }
}

#[test]
fn the_exiled_nonland_card_gains_the_ability_and_its_copy_can_be_cast() {
    let (mut runner, q, bolt, fodder) = q_scenario();
    runner
        .act(GameAction::CastSpell {
            object_id: q,
            card_id: runner.state().objects[&q].card_id,
            targets: vec![],
            payment_mode: CastPaymentMode::Auto,
        })
        .expect("Q is castable");
    settle(&mut runner);
    runner.advance_until_stack_empty();

    assert_eq!(
        runner.state().objects[&bolt].zone,
        Zone::Exile,
        "Q exiled the nonland card"
    );
    assert_eq!(runner.state().objects[&fodder].zone, Zone::Hand);

    // The granted ability is offered from exile.
    // CR 611.2a: no duration is stated, and the ability says it works on "your
    // turn" from exile — it must outlive this turn (it used to lapse at cleanup).
    assert!(
        runner
            .state()
            .transient_continuous_effects
            .iter()
            .any(
                |tce| tce.affected == TargetFilter::SpecificObject { id: bolt }
                    && tce.duration == Duration::Permanent
                    && tce.affected_recipient.is_some()
            ),
        "the grant lasts as long as the exiled card does: {:?}",
        runner.state().transient_continuous_effects
    );
    // A land exiled on the way gained nothing.
    let exiled_land = runner
        .state()
        .objects
        .values()
        .find(|o| o.zone == Zone::Exile && o.name == "Mountain")
        .map(|o| o.id)
        .expect("Q exiled the lands above the Bolt");
    assert!(
        engine::game::casting::activated_ability_definitions(runner.state(), exiled_land)
            .iter()
            .all(|(_, a)| !matches!(&*a.effect, Effect::CreateCardCopyByName { .. })),
        "only the nonland card gains the ability"
    );

    let index = engine::game::casting::activated_ability_definitions(runner.state(), bolt)
        .into_iter()
        .find(|(_, a)| matches!(&*a.effect, Effect::CreateCardCopyByName { .. }))
        .map(|(index, _)| index)
        .expect("the exiled card gained the ability");
    runner
        .act(GameAction::ActivateAbility {
            payment_mode: CastPaymentMode::Auto,
            source_id: bolt,
            ability_index: index,
        })
        .expect("the granted ability is activatable from exile on your turn");
    // Cost: exile a card from hand (the only one).
    for _ in 0..4 {
        if runner.state().objects[&fodder].zone == Zone::Exile {
            break;
        }
        match runner.state().waiting_for.clone() {
            WaitingFor::Priority { .. } => break,
            other => {
                runner
                    .act(GameAction::SelectCards {
                        cards: vec![fodder],
                    })
                    .unwrap_or_else(|e| panic!("paying the exile cost at {other:?}: {e:?}"));
            }
        }
    }
    assert_eq!(
        runner.state().objects[&fodder].zone,
        Zone::Exile,
        "the cost was paid"
    );
    runner.resolve_top();

    assert!(
        matches!(
            runner.state().waiting_for,
            WaitingFor::OptionalEffectChoice { .. }
        ),
        "\"you may cast the copy\" is asked: {:?}",
        runner.state().waiting_for
    );
    runner
        .act(GameAction::DecideOptionalEffect { accept: true })
        .expect("accept the cast");
    let hit = match &runner.state().waiting_for {
        WaitingFor::CastOffer {
            kind: CastOfferKind::GraveyardPaidCast { hit_card, .. },
            ..
        } => *hit_card,
        other => panic!("expected a during-resolution cast offer, got {other:?}"),
    };
    assert_ne!(hit, bolt, "the offer is over the COPY, not the exiled card");
    assert_eq!(runner.state().objects[&hit].name, "Lightning Bolt");
    assert!(runner.state().objects[&hit].is_copy);
    runner
        .act(GameAction::GraveyardPaidCastChoice {
            choice: CastChoice::Cast,
        })
        .expect("cast the copy");
    for _ in 0..6 {
        match runner.state().waiting_for {
            WaitingFor::ManaPayment { .. } => {
                runner.act(GameAction::PassPriority).expect("pay {R}");
            }
            WaitingFor::TargetSelection { .. } => {
                runner
                    .choose_first_legal_target()
                    .expect("a target for the Bolt");
            }
            _ => break,
        }
    }
    assert!(
        runner.stack_names().contains(&"Lightning Bolt".to_string()),
        "the copy is on the stack: {:?} / {:?}",
        runner.stack_names(),
        runner.state().waiting_for
    );
    // The original stays in exile, ability intact, for the next turn.
    assert_eq!(runner.state().objects[&bolt].zone, Zone::Exile);
}
