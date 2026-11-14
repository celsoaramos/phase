//! Runtime coverage for a PRINTED `Choose a color.` followed, in the same
//! ability, by a grant of "protection from the chosen color" (CR 608.2d +
//! CR 702.16).
//!
//! The printed chooser lowers `Effect::Choose { Color, persist: false }`, so it
//! writes no `ChosenAttribute::Color` onto its source. The grant's colour is read
//! from `GameState::chosen_color_this_resolution`
//! (`effects/choose.rs::resolution_chosen_color`), which used to be written only
//! for a PERSISTING chooser — the answer was asked, consumed and dropped, and the
//! white creatures got no protection at all.
//!
//! REVERT DISCRIMINATOR: restore the exact-object gate on the
//! `chosen_color_this_resolution` write in `effects/choose.rs::bind_named_choice`
//! and `resolution_chosen_color` returns `None`; `snapshot_transient_modifications`
//! leaves `Protection(ChosenColor)` unresolved, the layer applier skips it (the
//! spell has no chosen colour of its own), and every "has protection" assertion
//! below fails.

use engine::game::combat::{can_block_pair, validate_blockers, AttackTarget};
use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::types::actions::GameAction;
use engine::types::game_state::WaitingFor;
use engine::types::identifiers::ObjectId;
use engine::types::keywords::{Keyword, ProtectionTarget};
use engine::types::mana::{ManaColor, ManaCost, ManaCostShard, ManaType, ManaUnit};
use engine::types::phase::Phase;

/// Brave the Elements {W} Instant — verbatim Oracle text.
const BRAVE_THE_ELEMENTS: &str =
    "Choose a color. White creatures you control gain protection from the chosen color until end \
     of turn.";

/// Akroma's Blessing {2}{W} Instant — verbatim Oracle text, reminder included.
const AKROMAS_BLESSING: &str =
    "Choose a color. Creatures you control gain protection from the chosen color until end of \
     turn.\nCycling {W} ({W}, Discard this card: Draw a card.)";

/// A green non-targeted damage source, so CR 702.16e prevention is exercised
/// without CR 702.16b targeting getting in the way.
const GREEN_SWEEP: &str = "This spell deals 1 damage to each creature.";

fn mono(shard: ManaCostShard) -> ManaCost {
    ManaCost::Cost {
        shards: vec![shard],
        generic: 0,
    }
}

fn unit(mana: ManaType) -> ManaUnit {
    ManaUnit::new(mana, ObjectId(0), false, vec![])
}

/// Exact-colour check: `has_keyword` matches the `Protection` discriminant only.
fn protected_from(runner: &GameRunner, id: ObjectId, color: ManaColor) -> bool {
    runner.state().objects[&id]
        .keywords
        .contains(&Keyword::Protection(ProtectionTarget::Color(color)))
}

fn has_any_protection(runner: &GameRunner, id: ObjectId) -> bool {
    runner.state().objects[&id]
        .keywords
        .iter()
        .any(|k| matches!(k, Keyword::Protection(_)))
}

/// CR 608.2d + CR 702.16e + CR 702.16f + CR 611.2c + CR 514.2: Brave the
/// Elements naming green protects ONLY the white creatures its caster controls,
/// prevents green damage to them, makes a green blocker illegal, and ends at
/// cleanup.
#[test]
fn brave_the_elements_protects_white_creatures_you_control_until_end_of_turn() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let white = scenario
        .add_creature(P0, "White Lion", 2, 2)
        .with_mana_cost(mono(ManaCostShard::White))
        .id();
    let red = scenario
        .add_creature(P0, "Red Bear", 2, 2)
        .with_mana_cost(mono(ManaCostShard::Red))
        .id();
    let opp_white = scenario
        .add_creature(P1, "Opposing White Knight", 2, 2)
        .with_mana_cost(mono(ManaCostShard::White))
        .id();
    let green = scenario
        .add_creature(P1, "Green Bear", 2, 2)
        .with_mana_cost(mono(ManaCostShard::Green))
        .id();
    let brave = scenario
        .add_spell_to_hand_from_oracle(P0, "Brave the Elements", true, BRAVE_THE_ELEMENTS)
        .with_mana_cost(mono(ManaCostShard::White))
        .id();
    let sweep = scenario
        .add_spell_to_hand_from_oracle(P0, "Green Sweep", false, GREEN_SWEEP)
        .with_mana_cost(mono(ManaCostShard::Green))
        .id();
    scenario.with_mana_pool(P0, vec![unit(ManaType::White), unit(ManaType::Green)]);
    let mut runner = scenario.build();

    let cast = runner.cast(brave).choose_option("Green").resolve();
    assert!(
        matches!(cast.final_waiting_for(), WaitingFor::Priority { .. }),
        "Brave the Elements must resolve, got {:?}",
        cast.final_waiting_for()
    );

    // THE ASSERTION THAT FLIPS on revert.
    assert!(
        protected_from(&runner, white, ManaColor::Green),
        "the white creature you control must gain protection from GREEN: {:?}",
        runner.state().objects[&white].keywords
    );
    // Only the chosen colour.
    assert!(
        !protected_from(&runner, white, ManaColor::White),
        "the grant is from the chosen colour only"
    );
    // CR 611.2c: the affected set is white creatures YOU control.
    assert!(
        !has_any_protection(&runner, red),
        "a nonwhite creature you control gains nothing: {:?}",
        runner.state().objects[&red].keywords
    );
    assert!(
        !has_any_protection(&runner, opp_white),
        "an opponent's white creature gains nothing: {:?}",
        runner.state().objects[&opp_white].keywords
    );

    // CR 702.16e: damage from a green source to the protected creature is
    // prevented; the unprotected creature is the reach-guard.
    let swept = runner.cast(sweep).resolve();
    assert!(
        matches!(swept.final_waiting_for(), WaitingFor::Priority { .. }),
        "the green sweep must resolve, got {:?}",
        swept.final_waiting_for()
    );
    assert_eq!(
        runner.state().objects[&white].damage_marked,
        0,
        "CR 702.16e: green damage to the protected creature is prevented"
    );
    assert_eq!(
        runner.state().objects[&red].damage_marked,
        1,
        "reach-guard: the sweep really dealt damage to an unprotected creature"
    );

    // CR 702.16f: an attacking creature with protection from green can't be
    // blocked by a green creature.
    runner.advance_to_combat();
    runner
        .declare_attackers(&[(white, AttackTarget::Player(P1))])
        .expect("declare the white creature as an attacker (CR 508.1)");
    // CR 508.2: the declare-attackers step has a priority window before the
    // declare-blockers step begins.
    for _ in 0..8 {
        if matches!(
            runner.state().waiting_for,
            WaitingFor::DeclareBlockers { .. }
        ) {
            break;
        }
        runner
            .act(GameAction::PassPriority)
            .expect("pass priority into the declare-blockers step");
    }
    assert!(
        matches!(
            runner.state().waiting_for,
            WaitingFor::DeclareBlockers { .. }
        ),
        "the defending player must be asked to block, got {:?}",
        runner.state().waiting_for
    );
    assert!(
        !can_block_pair(runner.state(), green, white),
        "CR 702.16f: the green creature can't block the protected attacker"
    );
    assert!(
        validate_blockers(runner.state(), &[(green, white)]).is_err(),
        "CR 702.16f: declaring the green block is illegal"
    );
    assert!(
        validate_blockers(runner.state(), &[(opp_white, white)]).is_ok(),
        "reach-guard: a white creature may still block it"
    );

    // CR 514.2: "until end of turn" effects end in the cleanup step.
    runner
        .declare_blockers(&[])
        .expect("declare no blockers (CR 509.1)");
    runner.combat_damage();
    runner.advance_to_upkeep();
    assert_eq!(
        runner.state().phase,
        Phase::Upkeep,
        "the run must cross the cleanup step, got {:?}",
        runner.state().phase
    );
    assert!(
        !has_any_protection(&runner, white),
        "CR 514.2: the protection ends at cleanup: {:?}",
        runner.state().objects[&white].keywords
    );
}

/// Same class, different affected set: Akroma's Blessing protects EVERY
/// creature its caster controls, whatever its colour, and no opposing creature.
#[test]
fn akromas_blessing_protects_all_creatures_you_control_from_the_chosen_color() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let white = scenario
        .add_creature(P0, "White Lion", 2, 2)
        .with_mana_cost(mono(ManaCostShard::White))
        .id();
    let red = scenario
        .add_creature(P0, "Red Bear", 2, 2)
        .with_mana_cost(mono(ManaCostShard::Red))
        .id();
    let opposing = scenario
        .add_creature(P1, "Opposing Bear", 2, 2)
        .with_mana_cost(mono(ManaCostShard::Black))
        .id();
    let blessing = scenario
        .add_spell_to_hand_from_oracle(P0, "Akroma's Blessing", true, AKROMAS_BLESSING)
        .with_mana_cost(mono(ManaCostShard::White))
        .id();
    scenario.with_mana_pool(P0, vec![unit(ManaType::White)]);
    let mut runner = scenario.build();

    let cast = runner.cast(blessing).choose_option("Black").resolve();
    assert!(
        matches!(cast.final_waiting_for(), WaitingFor::Priority { .. }),
        "Akroma's Blessing must resolve, got {:?}",
        cast.final_waiting_for()
    );

    assert!(
        protected_from(&runner, white, ManaColor::Black),
        "white creature you control gains protection from BLACK: {:?}",
        runner.state().objects[&white].keywords
    );
    assert!(
        protected_from(&runner, red, ManaColor::Black),
        "red creature you control gains protection from BLACK too: {:?}",
        runner.state().objects[&red].keywords
    );
    assert!(
        !has_any_protection(&runner, opposing),
        "an opposing creature gains nothing: {:?}",
        runner.state().objects[&opposing].keywords
    );
}
