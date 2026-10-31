//! Relatos da mesa de 31/10/2026.
//!
//! Deekah, Fractal Theorist — "Magecraft — Whenever you cast or copy an instant
//! or sorcery spell, create a 0/0 green and blue Fractal creature token. Put X
//! +1/+1 counters on it, where X is that spell's mana value." The cast trigger
//! put the counters; the COPY trigger put none, and the 0/0 Fractal died.
//!
//! "That spell" in a copy trigger is the copy (CR 707.10: a copy of a spell is
//! itself a spell on the stack), and a copy has the original's mana cost and
//! therefore its mana value (CR 707.2 + CR 202.3). The `SpellCopied` event
//! named no subject for the mana-value reader, which answered 0.
//!
//! The Twincast case also locks the claim `card/mf-relatos-1028b` added in
//! `copy_spell`: a copy put on the stack by a resolving spell with nothing to
//! retarget must fire Magecraft once, not twice (CR 603.2).

use engine::game::scenario::{GameScenario, P0};
use engine::types::actions::GameAction;
use engine::types::counter::CounterType;
use engine::types::events::GameEvent;
use engine::types::game_state::{CastPaymentMode, GameState};
use engine::types::mana::{ManaCost, ManaCostShard, ManaType, ManaUnit};
use engine::types::phase::Phase;
use engine::types::zones::Zone;

const DEEKAH: &str = "Magecraft — Whenever you cast or copy an instant or sorcery spell, create a 0/0 green and blue Fractal creature token. Put X +1/+1 counters on it, where X is that spell's mana value.\n{3}{U}: Target creature token can't be blocked this turn.";
const TWINCAST: &str =
    "Copy target instant or sorcery spell. You may choose new targets for the copy.";

const ECHO_CREATURE: &str =
    "Whenever you cast an instant spell, copy that spell. You may choose new targets for the copy.";

fn fractals_created(events: &[GameEvent]) -> usize {
    events
        .iter()
        .filter(|e| matches!(e, GameEvent::TokenCreated { name, .. } if name == "Fractal"))
        .count()
}

fn mana(kinds: &[ManaType]) -> Vec<ManaUnit> {
    kinds
        .iter()
        .map(|kind| {
            ManaUnit::new(
                *kind,
                engine::types::identifiers::ObjectId(0),
                false,
                vec![],
            )
        })
        .collect()
}

fn fractal_counters(state: &GameState) -> Vec<u32> {
    let mut counters: Vec<u32> = state
        .battlefield
        .iter()
        .filter_map(|id| state.objects.get(id))
        .filter(|obj| obj.name == "Fractal")
        .map(|obj| {
            obj.counters
                .get(&CounterType::Plus1Plus1)
                .copied()
                .unwrap_or(0)
        })
        .collect();
    counters.sort_unstable();
    counters
}

/// The relato's shape: a trigger copies the spell (Kalamax, the Stormsire).
#[test]
fn deekah_trigger_copy_reads_the_copied_spells_mana_value() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario
        .add_creature_from_oracle(P0, "Deekah, Fractal Theorist", 3, 3, DEEKAH)
        .with_subtypes(vec!["Human", "Wizard"]);
    scenario.add_creature_from_oracle(P0, "Echo Witness", 2, 2, ECHO_CREATURE);
    let spell = scenario
        .add_spell_to_hand_from_oracle(P0, "Research Spell", true, "Draw a card.")
        .with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::Blue],
            generic: 2,
        })
        .id();
    scenario.with_mana_pool(
        P0,
        mana(&[ManaType::Blue, ManaType::Colorless, ManaType::Colorless]),
    );
    for _ in 0..4 {
        scenario.add_card_to_library_top(P0, "Island");
    }
    let mut runner = scenario.build();

    // Two cast triggers (Deekah and the copier) ask for an order; the runner
    // drains it in identity order.
    let card_id = runner.state().objects[&spell].card_id;
    runner
        .act(GameAction::CastSpell {
            object_id: spell,
            card_id,
            targets: Vec::new(),
            payment_mode: CastPaymentMode::Auto,
        })
        .expect("cast the MV-3 instant");
    runner.advance_until_stack_empty();

    // One Fractal for the cast, one for the copy — both with 3 (a double copy
    // trigger would leave a third).
    assert_eq!(
        fractal_counters(runner.state()),
        vec![3, 3],
        "the copy trigger must read the copy's mana value (CR 707.2)"
    );
    assert!(runner.state().stack.is_empty());
}

/// A resolving copy spell with nothing to retarget (Twincast).
#[test]
fn deekah_spell_copy_reads_mana_value_and_fires_once() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario
        .add_creature_from_oracle(P0, "Deekah, Fractal Theorist", 3, 3, DEEKAH)
        .with_subtypes(vec!["Human", "Wizard"]);
    // MV 3, no targets: the copy needs no retarget prompt.
    let spell = scenario
        .add_spell_to_hand_from_oracle(P0, "Research Spell", true, "Draw a card.")
        .with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::Blue],
            generic: 2,
        })
        .id();
    // MV 0: its own cast trigger makes a 0/0 that dies, which keeps the count
    // honest about which trigger produced which Fractal.
    let twincast = scenario
        .add_spell_to_hand_from_oracle(P0, "Twincast", true, TWINCAST)
        .with_mana_cost(ManaCost::zero())
        .id();
    scenario.with_mana_pool(
        P0,
        mana(&[ManaType::Blue, ManaType::Colorless, ManaType::Colorless]),
    );
    for _ in 0..4 {
        scenario.add_card_to_library_top(P0, "Island");
    }
    let mut runner = scenario.build();

    let mut original = runner.cast(spell).commit();
    let copied = original
        .cast(twincast)
        .target_object(spell)
        .commit()
        .resolve();
    let mut created = fractals_created(copied.events());
    created += fractals_created(original.resolve().events());
    runner.advance_until_stack_empty();

    // Cast of the MV-3 spell, Twincast's own cast, and ONE for the copy.
    assert_eq!(created, 3, "Magecraft fires once per copy (CR 603.2)");
    // MV-3 cast → 3. Twincast (MV 0) → 0/0, dies. Copy of the MV-3 spell → 3.
    assert_eq!(
        fractal_counters(runner.state()),
        vec![3, 3],
        "the copy trigger must read the copy's mana value (CR 707.2)"
    );
    assert_eq!(runner.state().objects[&spell].zone, Zone::Graveyard);
}
