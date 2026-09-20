//! Regression: **Fireblast** exiled by an impulse draw (Wrenn's Resolve,
//! Experimental Synthesizer) — "If you control two or more Mountains, you may
//! sacrifice two Mountains rather than pay this spell's mana cost."
//!
//! CR 118.9 + CR 601.2b: a spell's own printed alternative cost applies to any
//! cast that would otherwise pay its printed mana cost, not only to a cast from
//! hand. "You may play those cards" authorizes exactly that cast. Field report
//! (2026-09-20): with both Mountains tapped the exiled Fireblast was not
//! castable at all, while the same card in hand was.
//!
//! CR 118.9a: the offer must NOT reach a cast whose authority already replaces
//! the mana cost ("without paying its mana cost").

use engine::game::casting::can_cast_object_now;
use engine::game::scenario::{GameScenario, P0};
use engine::types::ability::{CastingPermission, ExileGrantCostProvenance};
use engine::types::actions::GameAction;
use engine::types::game_state::{CastPaymentMode, WaitingFor};
use engine::types::mana::{ManaColor, ManaCost, ManaCostShard, ManaType, ManaUnit};
use engine::types::phase::Phase;
use engine::types::zones::Zone;
use engine::types::ObjectId;

const FIREBLAST: &str = "If you control two or more Mountains, you may sacrifice two Mountains \
rather than pay this spell's mana cost.\nFireblast deals 4 damage to any target.";
const WRENNS_RESOLVE: &str = "Exile the top two cards of your library. Until the end of your next \
turn, you may play those cards.";

fn fireblast_cost() -> ManaCost {
    ManaCost::Cost {
        shards: vec![ManaCostShard::Red, ManaCostShard::Red],
        generic: 4,
    }
}

#[test]
fn fireblast_exiled_by_impulse_draw_offers_the_mountain_sacrifice() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let mountains = [
        scenario.add_basic_land(P0, ManaColor::Red),
        scenario.add_basic_land(P0, ManaColor::Red),
    ];
    // Two red mana pay for Wrenn's Resolve; nothing is left for {4}{R}{R}.
    scenario.with_mana_pool(
        P0,
        (0..2)
            .map(|_| ManaUnit::new(ManaType::Red, ObjectId(0), false, vec![]))
            .collect(),
    );
    let resolve = {
        let mut card =
            scenario.add_spell_to_hand_from_oracle(P0, "Wrenn's Resolve", false, WRENNS_RESOLVE);
        card.with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::Red],
            generic: 1,
        });
        card.id()
    };
    let filler = scenario.add_spell_to_library_top(P0, "Filler", false).id();
    let fireblast = {
        let mut card = scenario.add_spell_to_library_top(P0, "Fireblast", true);
        card.from_oracle_text(FIREBLAST);
        card.with_mana_cost(fireblast_cost());
        card.id()
    };
    let mut runner = scenario.build();
    for id in mountains {
        runner.state_mut().objects.get_mut(&id).unwrap().tapped = true;
    }

    runner.cast(resolve).resolve();
    let state = runner.state();
    assert_eq!(
        state.objects[&fireblast].zone,
        Zone::Exile,
        "Fireblast was exiled"
    );
    assert_eq!(state.objects[&filler].zone, Zone::Exile);
    assert!(state.players[0].mana_pool.mana.is_empty());

    assert!(
        can_cast_object_now(runner.state(), P0, fireblast),
        "two Mountains make the exiled Fireblast castable with no mana"
    );

    let card_id = runner.state().objects[&fireblast].card_id;
    runner
        .act(GameAction::CastSpell {
            object_id: fireblast,
            card_id,
            targets: vec![],
            payment_mode: CastPaymentMode::Auto,
        })
        .expect("announce Fireblast from exile");
    // Walk the cast to its end: target the opponent, take the alternative cost,
    // sacrifice both Mountains.
    for _ in 0..8 {
        let action = match &runner.state().waiting_for {
            WaitingFor::Priority { .. } => break,
            WaitingFor::OptionalCostChoice { .. } => GameAction::DecideOptionalCost { pay: true },
            other => engine::ai_support::legal_actions(runner.state())
                .into_iter()
                .find(|a| !matches!(a, GameAction::CancelCast))
                .unwrap_or_else(|| panic!("no legal action while waiting for {other:?}")),
        };
        runner.act(action).expect("cast step");
    }

    let state = runner.state();
    assert_eq!(
        state.stack.len(),
        1,
        "Fireblast is on the stack: {:?}",
        state.waiting_for
    );
    for id in mountains {
        assert_eq!(
            state.objects[&id].zone,
            Zone::Graveyard,
            "both Mountains were sacrificed"
        );
    }
}

#[test]
fn free_cast_from_exile_does_not_also_offer_the_printed_alternative_cost() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.add_basic_land(P0, ManaColor::Red);
    scenario.add_basic_land(P0, ManaColor::Red);
    let fireblast = {
        let mut card = scenario.add_spell_to_exile(P0, "Fireblast", true);
        card.from_oracle_text(FIREBLAST);
        card.with_mana_cost(fireblast_cost());
        card.id()
    };
    let mut runner = scenario.build();
    runner
        .state_mut()
        .objects
        .get_mut(&fireblast)
        .unwrap()
        .casting_permissions
        .push(CastingPermission::ExileWithAltCost {
            source_id: None,
            cost_provenance: ExileGrantCostProvenance::Alternative,
            cost: ManaCost::zero(),
            cast_transformed: false,
            constraint: None,
            granted_to: Some(P0),
            resolution_cleanup: None,
            duration: None,
            graveyard_replacement: None,
            mana_spend_permission: None,
            enters_with_counter: None,
            enters_with_modifications: Vec::new(),
            cast_cost_modifier: None,
        });

    let card_id = runner.state().objects[&fireblast].card_id;
    runner
        .act(GameAction::CastSpell {
            object_id: fireblast,
            card_id,
            targets: vec![],
            payment_mode: CastPaymentMode::Auto,
        })
        .expect("announce the free cast");
    for _ in 0..8 {
        let action = match &runner.state().waiting_for {
            WaitingFor::Priority { .. } => break,
            WaitingFor::OptionalCostChoice { .. } => {
                panic!("CR 118.9a: a free cast must not be offered a second alternative cost")
            }
            other => engine::ai_support::legal_actions(runner.state())
                .into_iter()
                .find(|a| !matches!(a, GameAction::CancelCast))
                .unwrap_or_else(|| panic!("no legal action while waiting for {other:?}")),
        };
        runner.act(action).expect("cast step");
    }
    let state = runner.state();
    assert_eq!(state.stack.len(), 1);
    assert_eq!(
        state
            .battlefield
            .iter()
            .filter(|id| state.objects[id].name == "Mountain")
            .count(),
        2,
        "no Mountain is sacrificed for a free cast"
    );
}
