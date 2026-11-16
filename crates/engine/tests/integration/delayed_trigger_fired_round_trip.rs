//! Undo/restore (`PersistedGameState`) must accept every state the live engine
//! produces. Three load checks rejected ordinary positions — every undo in that
//! window failed in the client:
//!
//! - a delayed trigger installed in an earlier step fires: its install command
//!   left the rules journal (truncated at every step) and the one-shot trigger
//!   left `delayed_triggers` (CR 603.7c), so the fired carrier had "no install
//!   root" (Mishra's Bauble's next-upkeep draw, Kav Landseeker's sacrifice);
//! - a target slot's P/T direction lost its tag on export (`unknown variant
//!   Increase`) while choosing the target of any pump;
//! - creatures that left together carry `co_departed` on their events but not
//!   on their ledger rows (CR 603.10a), so their dies triggers had "no ledger
//!   occurrence".

use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::types::actions::GameAction;
use engine::types::game_state::{
    PersistedGameState, PersistedRestoreFinalization, StackEntryKind, WaitingFor,
};
use engine::types::identifiers::ObjectId;
use engine::types::mana::ManaCost;
use engine::types::phase::Phase;

/// Kav Landseeker's exact Oracle text: its ETB arms a delayed sacrifice for the
/// end step of the controller's NEXT turn (CR 603.7a), so the trigger fires two
/// turns after it was installed.
const KAV_ORACLE: &str = "Menace (This creature can't be blocked except by two or more creatures.)\n\
     When this creature enters, create a Lander token. At the beginning of the end step on your next turn, sacrifice that token. (It's an artifact with \"{2}, {T}, Sacrifice this token: Search your library for a basic land card, put it onto the battlefield tapped, then shuffle.\")";

fn kavs_in_hand(count: usize) -> (GameRunner, Vec<ObjectId>) {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.with_library_top(P0, &["Forest", "Forest", "Forest", "Forest", "Forest"]);
    scenario.with_library_top(P1, &["Forest", "Forest", "Forest", "Forest", "Forest"]);
    let kavs = (0..count)
        .map(|_| {
            let mut builder = scenario.add_creature_to_hand(P0, "Kav Landseeker", 4, 3);
            builder.from_oracle_text_with_keywords(&["Menace"], KAV_ORACLE);
            builder.with_mana_cost(ManaCost::generic(0));
            builder.id()
        })
        .collect();
    (scenario.build(), kavs)
}

/// The engine's own save/undo/P2P restore pipeline (`PersistedGameState`).
fn round_trip(runner: &GameRunner) -> GameRunner {
    let json = serde_json::to_string(&PersistedGameState::capture(runner.state().clone()))
        .expect("state serializes");
    let state = serde_json::from_str::<PersistedGameState>(&json)
        .expect("persisted state decodes")
        .prepare_for_restore(PersistedRestoreFinalization::DeferUntilRehydrated)
        .expect("persisted state is admissible")
        .finalize_after_rehydration(|_| Ok(()))
        .expect("restored state is publishable");
    GameRunner::from_state(state)
}

/// Advance through the real turn machinery until the delayed trigger fired on
/// `turn` (it sits on the stack or in the trigger-order prompt).
fn advance_until_fired(runner: &mut GameRunner, turn: u32) {
    for _ in 0..400 {
        let state = runner.state();
        let fired = state.turn_number == turn
            && (matches!(state.waiting_for, WaitingFor::OrderTriggers { .. })
                || state
                    .stack
                    .iter()
                    .any(|entry| matches!(entry.kind, StackEntryKind::TriggeredAbility { .. })));
        if fired {
            return;
        }
        match runner.state().waiting_for.clone() {
            WaitingFor::DeclareAttackers { .. } => {
                runner
                    .act(GameAction::DeclareAttackers {
                        attacks: vec![],
                        bands: vec![],
                    })
                    .expect("declaring no attackers must be accepted");
            }
            WaitingFor::DeclareBlockers { .. } => {
                runner
                    .act(GameAction::DeclareBlockers {
                        assignments: vec![],
                    })
                    .expect("declaring no blockers must be accepted");
            }
            WaitingFor::DiscardChoice { .. } => {
                runner
                    .act(GameAction::SelectCards { cards: vec![] })
                    .expect("no-op cleanup discard must be accepted");
            }
            WaitingFor::Priority { .. } => {
                runner
                    .act(GameAction::PassPriority)
                    .expect("passing priority must be accepted");
            }
            other => panic!("unexpected WaitingFor while advancing turns: {other:?}"),
        }
    }
    panic!("the delayed trigger never fired on turn {turn}");
}

#[test]
fn fired_delayed_trigger_from_an_earlier_turn_round_trips_on_the_stack() {
    let (mut runner, kavs) = kavs_in_hand(1);
    runner.cast(kavs[0]).resolve();
    advance_until_fired(&mut runner, 4);
    assert!(
        runner
            .state()
            .stack
            .iter()
            .any(|entry| matches!(entry.kind, StackEntryKind::TriggeredAbility { .. })),
        "the delayed sacrifice must be on the stack"
    );

    let mut restored = round_trip(&runner);
    restored.advance_until_stack_empty();
    assert!(
        !restored
            .state()
            .objects
            .values()
            .any(|o| o.name == "Lander" && o.zone == engine::types::zones::Zone::Battlefield),
        "the restored delayed trigger still resolves and sacrifices the Lander"
    );
}

#[test]
fn fired_delayed_triggers_from_an_earlier_turn_round_trip_in_the_order_prompt() {
    let (mut runner, kavs) = kavs_in_hand(2);
    for kav in kavs {
        runner.cast(kav).resolve();
    }
    advance_until_fired(&mut runner, 4);
    assert!(
        matches!(runner.state().waiting_for, WaitingFor::OrderTriggers { .. }),
        "two delayed sacrifices firing together ask their controller for an order"
    );

    round_trip(&runner);
}

/// A target slot's P/T direction (`Effect::Pump` "+N/+N" / "-N/-N") must
/// survive export: the inner `type` tag of the direction used to collide with
/// the outer one and the export read `{"type":"Increase"}`, which no longer
/// decoded ("unknown variant `Increase`") — undo failed while choosing the
/// target of every pump spell.
#[test]
fn target_slot_pump_direction_survives_export() {
    use engine::types::game_state::{PtDirection, TargetEffectDetail};
    for direction in [PtDirection::Increase, PtDirection::Decrease] {
        let detail = TargetEffectDetail::Modification(direction);
        // The persisted state goes through `serde_json::Value`, whose map keeps
        // one value per key.
        let value = serde_json::to_value(detail).expect("detail serializes");
        let back: TargetEffectDetail = serde_json::from_value(value).expect("detail decodes");
        assert_eq!(back, detail);
    }
    // States exported before the fix carry the collapsed form; they decode to
    // the direction they lost the outer tag of.
    let legacy: TargetEffectDetail =
        serde_json::from_str(r#"{"type":"Decrease"}"#).expect("legacy form decodes");
    assert_eq!(
        legacy,
        TargetEffectDetail::Modification(PtDirection::Decrease)
    );
}

