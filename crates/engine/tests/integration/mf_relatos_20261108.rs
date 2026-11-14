//! Field reports 2026-11-08 (MagicFinder mesa).
//!
//! 1. Sleep — "Tap all creatures target player controls. They don't untap
//!    during that player's next untap step." — the creatures untapped on the
//!    opponent's very next untap step (CR 502.3).

use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::types::game_state::WaitingFor;
use engine::types::phase::Phase;
use engine::types::player::PlayerId;

/// Pass through turns (declaring no attackers/blockers) until `player`'s upkeep.
fn advance_to_upkeep_of(runner: &mut GameRunner, player: PlayerId) {
    for _ in 0..12 {
        match runner.state().waiting_for {
            WaitingFor::DeclareAttackers { .. } => {
                runner.declare_attackers(&[]).expect("no attackers");
                continue;
            }
            WaitingFor::DeclareBlockers { .. } => {
                runner.declare_blockers(&[]).expect("no blockers");
                continue;
            }
            _ => {}
        }
        if runner.state().active_player == player && runner.state().phase == Phase::Upkeep {
            return;
        }
        if runner.state().phase == Phase::Upkeep {
            runner.advance_to_end_step();
        } else {
            runner.advance_to_upkeep();
        }
    }
    panic!(
        "never reached {player:?}'s upkeep (waiting: {:?}, phase: {:?})",
        runner.state().waiting_for,
        runner.state().phase
    );
}

const SLEEP: &str = "Tap all creatures target player controls. They don't untap during that \
player's next untap step.";

#[test]
fn sleep_creatures_stay_tapped_through_next_untap_step() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let bear = scenario.add_creature(P1, "Grizzly Bears", 2, 2).id();
    let giant = scenario.add_creature(P1, "Hill Giant", 3, 3).id();
    // Already tapped before Sleep (it attacked last turn): "They" covers every
    // creature that player controls, not only the ones Sleep tapped.
    let attacker = scenario.add_creature(P1, "Vampire Nighthawk", 2, 3).id();
    let mine = scenario.add_creature(P0, "Coralhelm Guide", 2, 1).id();
    // Cards to draw, so neither player decks out across the three turns.
    scenario.with_library_top(P0, &["Island", "Island", "Island"]);
    scenario.with_library_top(P1, &["Swamp", "Swamp", "Swamp"]);
    let spell = scenario
        .add_spell_to_hand_from_oracle(P0, "Sleep", false, SLEEP)
        .id();
    let mut runner = scenario.build();
    runner
        .state_mut()
        .objects
        .get_mut(&attacker)
        .unwrap()
        .tapped = true;
    runner.state_mut().objects.get_mut(&mine).unwrap().tapped = true;

    runner.cast(spell).target_player(P1).resolve();
    for id in [bear, giant, attacker] {
        assert!(runner.state().objects[&id].tapped, "Sleep taps all of them");
    }

    advance_to_upkeep_of(&mut runner, P1);
    for id in [bear, giant, attacker] {
        assert!(
            runner.state().objects[&id].tapped,
            "CR 502.3: {} must not untap during its controller's next untap step",
            runner.state().objects[&id].name
        );
    }

    // The lock is for ONE untap step only: on P1's following turn they untap.
    advance_to_upkeep_of(&mut runner, P0);
    assert!(
        !runner.state().objects[&mine].tapped,
        "Sleep never touches the caster's creatures"
    );
    advance_to_upkeep_of(&mut runner, P1);
    for id in [bear, giant, attacker] {
        assert!(
            !runner.state().objects[&id].tapped,
            "{} untaps on the untap step after the locked one",
            runner.state().objects[&id].name
        );
    }
}

const CREEPING_CHILL: &str =
    "Creeping Chill deals 3 damage to each opponent and you gain 3 life.\n\
When Creeping Chill is put into your graveyard from your library, you may exile it. If you do, \
Creeping Chill deals 3 damage to each opponent and you gain 3 life.";

/// Report 2: "the second effect of Creeping Chill doesn't work". The spell cast
/// from hand already did its 3 + 3 in the report's log; the second ability
/// triggers only when the card is MILLED (library → graveyard), never when the
/// resolved spell goes to the graveyard (CR 608.2n). Guard that the milled
/// trigger does work.
#[test]
fn creeping_chill_milled_from_library_drains_when_exiled() {
    use engine::types::zones::Zone;

    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let chill = scenario
        .add_spell_to_library_top(P0, "Creeping Chill", false)
        .from_oracle_text(CREEPING_CHILL)
        .id();
    let mill = scenario
        .add_spell_to_hand_from_oracle(P0, "Thought Scour", true, "Target player mills two cards.")
        .id();
    let mut runner = scenario.build();
    let (me, them) = (runner.life(P0), runner.life(P1));

    runner
        .cast(mill)
        .target_player(P0)
        .accept_optional()
        .resolve();

    assert_eq!(
        runner.state().objects[&chill].zone,
        Zone::Exile,
        "Creeping Chill was exiled"
    );
    assert_eq!(runner.life(P1), them - 3, "3 damage to each opponent");
    assert_eq!(runner.life(P0), me + 3, "you gain 3 life");
}
