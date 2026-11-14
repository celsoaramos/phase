//! Lady Octopus, Inspired Inventor — "Whenever you draw your first or second
//! card each turn, put an ingenuity counter on Lady Octopus."
//!
//! Drives the REAL parse → trigger → stack pipeline: Lady Octopus is built from
//! Oracle text via the scenario harness, draws go through `draw::resolve`, and
//! the trigger resolves off the stack.
//!
//! THE BUG this discriminates: the ordinal payload "first or second" was not
//! understood by the nth-draw trigger parser, so the whole trigger fell through
//! to `TriggerMode::Unknown` and never fired — no ingenuity counter on any draw
//! (field report from the MagicFinder rules table). The fix parses the
//! leading-run disjunction as `NthDrawThisTurn { n: 2, comparator: LE }`
//! (CR 121.2: each card drawn is a separate draw event with its own ordinal).
//! (a) the first draw fires, (b) the second draw fires, (c) the third draw of
//! the same turn does NOT — the last one discriminates `LE 2` from a
//! fire-on-every-draw misparse.

use engine::game::effects::draw::resolve as resolve_draw;
use engine::game::scenario::{GameRunner, GameScenario};
use engine::game::stack::resolve_top;
use engine::game::triggers::process_triggers;
use engine::types::ability::{Effect, QuantityExpr, ResolvedAbility, TargetFilter};
use engine::types::counter::CounterType;
use engine::types::identifiers::ObjectId;
use engine::types::phase::Phase;
use engine::types::player::PlayerId;

const P0: PlayerId = PlayerId(0);

const LADY_OCTOPUS: &str = "Whenever you draw your first or second card each turn, put an ingenuity counter on Lady Octopus.\n\
{T}: You may cast an artifact spell from your hand with mana value less than or equal to the number of ingenuity counters on Lady Octopus without paying its mana cost.";

/// Resolve one draw for P0 through the production `draw::resolve` seam, then
/// process the resulting triggers.
fn draw_one(runner: &mut GameRunner) {
    let ability = ResolvedAbility::new(
        Effect::Draw {
            count: QuantityExpr::Fixed { value: 1 },
            target: TargetFilter::Controller,
        },
        Vec::new(),
        ObjectId(0),
        P0,
    );
    let mut events = Vec::new();
    resolve_draw(runner.state_mut(), &ability, &mut events).expect("draw resolves");
    process_triggers(runner.state_mut(), &events);
}

fn ingenuity(runner: &GameRunner, id: ObjectId) -> u32 {
    runner.state().objects[&id]
        .counters
        .get(&CounterType::Generic("ingenuity".to_string()))
        .copied()
        .unwrap_or(0)
}

fn resolve_stack(runner: &mut GameRunner) {
    while !runner.state().stack.is_empty() {
        let mut events = Vec::new();
        resolve_top(runner.state_mut(), &mut events);
    }
}

#[test]
fn lady_octopus_counts_first_and_second_draw_only() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let lady = scenario
        .add_creature_from_oracle(P0, "Lady Octopus, Inspired Inventor", 0, 2, LADY_OCTOPUS)
        .id();
    for i in 0..4 {
        scenario.add_card_to_library_top(P0, &format!("Library Card {i}"));
    }
    let mut runner = scenario.build();
    assert_eq!(ingenuity(&runner, lady), 0);

    // (a) First draw of the turn fires.
    draw_one(&mut runner);
    assert_eq!(
        runner.state().stack.len(),
        1,
        "the first draw fires 'your first or second card each turn'"
    );
    resolve_stack(&mut runner);
    assert_eq!(ingenuity(&runner, lady), 1);

    // (b) Second draw fires again.
    draw_one(&mut runner);
    assert_eq!(
        runner.state().stack.len(),
        1,
        "the second draw fires 'your first or second card each turn'"
    );
    resolve_stack(&mut runner);
    assert_eq!(ingenuity(&runner, lady), 2);

    // (c) Third draw of the same turn does NOT fire.
    draw_one(&mut runner);
    assert_eq!(
        runner.state().stack.len(),
        0,
        "the third draw is outside 'first or second'"
    );
    assert_eq!(ingenuity(&runner, lady), 2);
}
