//! Regression: **Carrot Cake** (BLB) — "When this artifact enters and when you
//! sacrifice it, create a 1/1 white Rabbit creature token and scry 1.
//! {2}, {T}, Sacrifice this artifact: You gain 3 life."
//!
//! CR 603.10a: a "when you sacrifice ~" trigger looks back in time, so it must
//! fire even though its own source is already in the graveyard once the
//! sacrifice (here: an activation COST, CR 602.2b + CR 118.3) is done.
//! Field report (2026-09-19): the Rabbit only came from the ETB half.

use engine::game::scenario::{GameScenario, P0};
use engine::types::actions::GameAction;
use engine::types::mana::{ManaType, ManaUnit};
use engine::types::phase::Phase;
use engine::types::zones::Zone;
use engine::types::ObjectId;

const CARROT_CAKE: &str = "When this artifact enters and when you sacrifice it, create a 1/1 white \
Rabbit creature token and scry 1. (Look at the top card of your library. You may put that card on the bottom.)\n\
{2}, {T}, Sacrifice this artifact: You gain 3 life.";

#[test]
fn carrot_cake_sacrificed_as_own_cost_triggers() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.with_mana_pool(
        P0,
        (0..2)
            .map(|_| ManaUnit::new(ManaType::White, ObjectId(0), false, vec![]))
            .collect(),
    );
    let cake = scenario
        .add_artifact_from_oracle(P0, "Carrot Cake", CARROT_CAKE)
        .id();
    let mut runner = scenario.build();

    let ability_index = runner.state().objects[&cake]
        .abilities
        .iter()
        .position(|a| a.cost.is_some())
        .expect("the {2}, {T}, Sacrifice activated ability");
    runner
        .act(GameAction::ActivateAbility {
            source_id: cake,
            ability_index,
        })
        .expect("activate");

    let state = runner.state();
    assert_ne!(
        state.objects.get(&cake).map(|o| o.zone),
        Some(Zone::Battlefield),
        "the cake was sacrificed as part of the cost"
    );
    assert_eq!(
        state.stack.len(),
        2,
        "the life-gain ability AND the 'when you sacrifice it' trigger must both be on the stack; got {:?}",
        state.stack
    );

    let life_before = runner.state().players[0].life;
    runner.resolve_top();
    runner.resolve_top();
    let state = runner.state();
    let rabbits = state
        .battlefield
        .iter()
        .filter(|id| state.objects[id].name == "Rabbit" && state.objects[id].controller == P0)
        .count();
    assert_eq!(rabbits, 1, "the sacrifice trigger creates the Rabbit");
    assert_eq!(
        state.players[0].life,
        life_before + 3,
        "the ability still gains 3 life"
    );
}
