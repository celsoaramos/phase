//! Field reports 2026-11-07 (MagicFinder mesa).
//!
//! 1. The Mightstone and Weakstone — "{T}: Add {C}{C}. This mana can't be spent
//!    to cast nonartifact spells." — could not pay Ultron, Artificial
//!    Malevolence's "you may pay {2}". CR 106.6: the negative restriction only
//!    forbids casting nonartifact spells; a cost paid while a triggered ability
//!    resolves is not a spell cast. The parser lowered it to "only artifact
//!    spells or ability activations", which also rejected resolution-time
//!    payments, so accepting the trigger paid nothing and created no token.

use engine::game::scenario::{GameRunner, GameScenario, P0};
use engine::game::scenario_db::GameScenarioDbExt;
use engine::types::actions::GameAction;
use engine::types::game_state::WaitingFor;
use engine::types::identifiers::ObjectId;
use engine::types::mana::{AbilityActivationScope, ManaRestriction, PaymentContext, SpellMeta};
use engine::types::phase::Phase;
use engine::types::zones::Zone;

use crate::support::shared_card_db;

const MIGHTSTONE: &str = "The Mightstone and Weakstone";
const ULTRON: &str = "Ultron, Artificial Malevolence";
fn cast(runner: &mut GameRunner, object_id: ObjectId) {
    let card_id = runner.state().objects[&object_id].card_id;
    runner
        .act(GameAction::CastSpell {
            object_id,
            card_id,
            targets: vec![],
            payment_mode: Default::default(),
        })
        .expect("cast");
}

fn tokens(runner: &GameRunner) -> usize {
    runner
        .state()
        .battlefield
        .iter()
        .filter(|id| runner.state().objects[id].is_token)
        .count()
}

/// Drives Ultron's trigger: accept "you may pay {2}", let the engine pay it.
fn resolve_ultron_trigger(runner: &mut GameRunner) {
    for _ in 0..24 {
        match runner.state().waiting_for.clone() {
            WaitingFor::OptionalEffectChoice { .. } => {
                runner
                    .act(GameAction::DecideOptionalEffect { accept: true })
                    .expect("accept Ultron");
            }
            WaitingFor::ManaPayment { .. } => {
                runner.act(GameAction::PassPriority).expect("pay {2}");
            }
            WaitingFor::Priority { .. } if !runner.state().stack.is_empty() => {
                runner.act(GameAction::PassPriority).expect("pass");
            }
            _ => return,
        }
    }
}

#[test]
fn mightstone_mana_pays_ultrons_resolution_cost() {
    let db = shared_card_db().expect("integration card fixture must load");
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let stone = scenario.add_real_card(P0, MIGHTSTONE, Zone::Battlefield, db);
    scenario.add_real_card(P0, ULTRON, Zone::Battlefield, db);
    let relic = scenario
        .add_artifact_to_hand_from_oracle(P0, "Inert Relic", "")
        .id();
    let mut runner = scenario.build();

    cast(&mut runner, relic);
    resolve_ultron_trigger(&mut runner);

    assert!(
        runner.state().objects[&stone].tapped,
        "the Mightstone's {{C}}{{C}} is the only mana that can pay Ultron's {{2}}"
    );
    assert_eq!(
        tokens(&runner),
        1,
        "CR 106.6: mana that can't be spent on nonartifact SPELLS still pays a cost \
         paid while a triggered ability resolves — Ultron must copy the artifact"
    );
}

#[test]
fn ultron_without_mana_creates_no_token() {
    // Reach guard for the test above: with no mana source at all, accepting
    // the trigger cannot pay {2}, so no token.
    let db = shared_card_db().expect("integration card fixture must load");
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.add_real_card(P0, ULTRON, Zone::Battlefield, db);
    let relic = scenario
        .add_artifact_to_hand_from_oracle(P0, "Inert Relic", "")
        .id();
    let mut runner = scenario.build();

    cast(&mut runner, relic);
    resolve_ultron_trigger(&mut runner);

    assert_eq!(tokens(&runner), 0);
}

fn spell_meta(types: &[&str]) -> SpellMeta {
    SpellMeta {
        types: types.iter().map(|t| t.to_string()).collect(),
        subtypes: vec![],
        keyword_kinds: vec![],
        cast_from_zone: Some(Zone::Hand),
        mana_value: None,
        color_count: None,
        colors: vec![],
        has_x_in_cost: false,
        is_face_down: false,
        cant_spend_mana: false,
        spend_only_on_x_colors: None,
        spend_only_on_x_generic_count: 0,
    }
}

#[test]
fn negative_spell_type_restriction_only_gates_spell_casts() {
    // CR 106.6: "can't be spent to cast nonartifact spells".
    let negative = ManaRestriction::OnlyForTypeSpellsOrAbilities {
        spell_type: "Artifact".to_string(),
        ability: AbilityActivationScope::NonSpellUnrestricted,
    };
    assert!(negative.allows(&PaymentContext::Spell(&spell_meta(&["Artifact"]))));
    assert!(!negative.allows(&PaymentContext::Spell(&spell_meta(&["Instant"]))));
    assert!(negative.allows(&PaymentContext::Effect));

    // The positive "only to cast an artifact spell or to activate an ability"
    // reading keeps rejecting resolution-time payments.
    let positive = ManaRestriction::OnlyForTypeSpellsOrAbilities {
        spell_type: "Artifact".to_string(),
        ability: AbilityActivationScope::Any,
    };
    assert!(!positive.allows(&PaymentContext::Effect));
    assert!(!positive.allows(&PaymentContext::Spell(&spell_meta(&["Instant"]))));
}
