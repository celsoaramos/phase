//! Field reports 2026-11-07 (MagicFinder mesa).
//!
//! 1. The Mightstone and Weakstone — "{T}: Add {C}{C}. This mana can't be spent
//!    to cast nonartifact spells." — could not pay Ultron, Artificial
//!    Malevolence's "you may pay {2}". CR 106.6: the negative restriction only
//!    forbids casting nonartifact spells; a cost paid while a triggered ability
//!    resolves is not a spell cast. The parser lowered it to "only artifact
//!    spells or ability activations", which also rejected resolution-time
//!    payments, so accepting the trigger paid nothing and created no token.
//! 2. The Mimeoplasm entering as a copy of Archon of Cruelty did not trigger
//!    Archon's "Whenever Archon of Cruelty enters or attacks". With three or more
//!    creature cards in graveyards the exile cost is an interactive choice; the
//!    resumed replacement finished the entry, but only the exile cost's events
//!    were collected for triggers (CR 603.6a + CR 614.12a + CR 707.2).

use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
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
const ARCHON: &str = "Archon of Cruelty";
const MIMEOPLASM: &str =
    "As ~ enters, you may exile two creature cards from graveyards. If you do, \
it enters as a copy of one of those cards with a number of additional +1/+1 counters on it equal \
to the power of the other card.";

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

/// Stack entries whose source is `source` (the Mimeoplasm-as-Archon trigger).
fn triggers_on_stack_from(runner: &GameRunner, source: ObjectId) -> usize {
    runner
        .state()
        .stack
        .iter()
        .filter(|entry| entry.source_id == source)
        .count()
}

/// Casts The Mimeoplasm with `extra_creatures` creature cards beside Archon and
/// the Grizzly Bears in P0's graveyard, exiles Archon (copied) and the Bears
/// (counters), and stops at the first priority after it enters.
fn mimeoplasm_enters_as_archon(extra_creatures: usize) -> (GameRunner, ObjectId) {
    let db = shared_card_db().expect("integration card fixture must load");
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let archon = scenario.add_real_card(P0, ARCHON, Zone::Graveyard, db);
    let bears = scenario
        .add_creature_to_graveyard(P0, "Grizzly Bears", 2, 2)
        .id();
    for i in 0..extra_creatures {
        scenario.add_creature_to_graveyard(P0, &format!("Hill Giant {i}"), 3, 3);
    }
    scenario.add_creature(P1, "Opposing Bear", 2, 2);
    let mimeo = scenario
        .add_creature_to_hand_from_oracle(P0, "The Mimeoplasm", 0, 0, MIMEOPLASM)
        .id();
    let mut runner = scenario.build();

    cast(&mut runner, mimeo);
    for _ in 0..16 {
        if runner.state().objects[&mimeo].zone == Zone::Battlefield
            && matches!(runner.state().waiting_for, WaitingFor::Priority { .. })
        {
            break;
        }
        match runner.state().waiting_for.clone() {
            WaitingFor::ReplacementChoice { .. } => {
                runner
                    .act(GameAction::ChooseReplacement { index: 0 })
                    .expect("accept the exile");
            }
            WaitingFor::EffectZoneChoice { .. } => {
                runner
                    .act(GameAction::SelectCards {
                        cards: vec![archon, bears],
                    })
                    .expect("exile Archon and the Bears");
            }
            WaitingFor::Priority { .. } => {
                runner.act(GameAction::PassPriority).expect("pass");
            }
            other => panic!("unexpected prompt {other:?}"),
        }
    }
    let obj = &runner.state().objects[&mimeo];
    assert_eq!(obj.zone, Zone::Battlefield);
    assert_eq!(obj.name, ARCHON, "CR 707.2: it enters as a copy of Archon");
    (runner, mimeo)
}

#[test]
fn mimeoplasm_interactive_exile_copy_of_archon_triggers_on_entering() {
    let (runner, mimeo) = mimeoplasm_enters_as_archon(1);
    assert_eq!(
        triggers_on_stack_from(&runner, mimeo),
        1,
        "CR 603.6a + CR 707.2: entering as a copy of Archon of Cruelty triggers its \
         enters ability exactly once, also when the exile cost was chosen"
    );
}

#[test]
fn mimeoplasm_forced_exile_copy_of_archon_triggers_once() {
    // Only two creature cards: the exile is forced (no choice). Guard that the
    // already-working path still triggers exactly once.
    let (runner, mimeo) = mimeoplasm_enters_as_archon(0);
    assert_eq!(triggers_on_stack_from(&runner, mimeo), 1);
}
