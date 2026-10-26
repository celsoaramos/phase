//! Relatos da mesa do MagicFinder de 26/10/2026.
//!
//! Firespitter Whelp — "Whenever you cast a noncreature or Dragon spell, this
//! creature deals 1 damage to each opponent." Conjurar um Dragão-criatura não
//! causava o dano.
//!
//! Niv-Mizzet, Ghost Counsel — "Whenever you gain life, you may pay that much
//! life. If you do, draw that many cards." Ganhar 1 de vida pela própria
//! habilidade {T} não oferecia pagar vida para comprar.

use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::types::actions::GameAction;
use engine::types::game_state::{CastPaymentMode, WaitingFor};
use engine::types::identifiers::ObjectId;
use engine::types::phase::Phase;
use engine::types::zones::Zone;

const FIRESPITTER_WHELP: &str = "Flying\nWhenever you cast a noncreature or Dragon spell, this \
                                 creature deals 1 damage to each opponent.";

const NIV_MIZZET_GHOST_COUNSEL: &str = "Flying\nWhenever you gain life, you may pay that much \
                                        life. If you do, draw that many cards.\n{T}: Each \
                                        opponent loses 1 life and you gain 1 life.";

fn cast(runner: &mut GameRunner, id: ObjectId) {
    let card_id = runner.state().objects[&id].card_id;
    runner
        .act(GameAction::CastSpell {
            object_id: id,
            card_id,
            targets: vec![],
            payment_mode: CastPaymentMode::Auto,
        })
        .expect("cast must succeed");
}

fn whelp_scenario() -> (GameScenario, ObjectId) {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let whelp = scenario
        .add_creature_from_oracle(P0, "Firespitter Whelp", 2, 2, FIRESPITTER_WHELP)
        .with_subtypes(vec!["Dragon"])
        .id();
    (scenario, whelp)
}

fn whelp_trigger_debug(runner: &GameRunner, whelp: ObjectId) -> String {
    format!("{:#?}", runner.state().objects[&whelp].trigger_definitions)
}

/// CR 601.2 + CR 603.2: casting a Dragon CREATURE spell matches the "Dragon"
/// leg of "noncreature or Dragon spell" — the "non" prefix negates only
/// "creature", not the whole disjunction (CR 205.4b).
#[test]
fn firespitter_whelp_pings_when_casting_a_dragon_creature_spell() {
    let (mut scenario, whelp) = whelp_scenario();
    let dragon = scenario
        .add_creature_to_hand_from_oracle(P0, "Firespitter Whelp", 2, 2, FIRESPITTER_WHELP)
        .with_subtypes(vec!["Dragon"])
        .id();
    let mut runner = scenario.build();
    let before = runner.life(P1);

    cast(&mut runner, dragon);
    runner.advance_until_stack_empty();

    assert_eq!(runner.state().objects[&dragon].zone, Zone::Battlefield);
    assert_eq!(
        runner.life(P1),
        before - 1,
        "casting a Dragon spell must make Firespitter Whelp deal 1 damage to each opponent; \
         triggers: {}",
        whelp_trigger_debug(&runner, whelp),
    );
}

#[test]
fn firespitter_whelp_pings_when_casting_a_noncreature_spell() {
    let (mut scenario, whelp) = whelp_scenario();
    let spell = scenario
        .add_spell_to_hand_from_oracle(P0, "Opt", true, "Scry 1.")
        .id();
    let mut runner = scenario.build();
    let before = runner.life(P1);

    cast(&mut runner, spell);
    runner.advance_until_stack_empty();

    assert_eq!(
        runner.life(P1),
        before - 1,
        "casting a noncreature spell must trigger Firespitter Whelp; triggers: {}",
        whelp_trigger_debug(&runner, whelp),
    );
}

#[test]
fn firespitter_whelp_ignores_a_non_dragon_creature_spell() {
    let (mut scenario, _whelp) = whelp_scenario();
    let bear = scenario
        .add_creature_to_hand_from_oracle(P0, "Grizzly Bears", 2, 2, "")
        .with_subtypes(vec!["Bear"])
        .id();
    let mut runner = scenario.build();
    let before = runner.life(P1);

    cast(&mut runner, bear);
    runner.advance_until_stack_empty();

    assert_eq!(runner.state().objects[&bear].zone, Zone::Battlefield);
    assert_eq!(
        runner.life(P1),
        before,
        "a non-Dragon creature spell is neither noncreature nor Dragon"
    );
}

fn niv_scenario() -> (GameRunner, ObjectId) {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let niv = scenario
        .add_creature_from_oracle(
            P0,
            "Niv-Mizzet, Ghost Counsel",
            4,
            4,
            NIV_MIZZET_GHOST_COUNSEL,
        )
        .as_legendary()
        .with_subtypes(vec!["Spirit", "Dragon"])
        .id();
    scenario.with_library_top(P0, &["Island", "Plains", "Swamp"]);
    (scenario.build(), niv)
}

fn activate_niv(runner: &mut GameRunner, niv: ObjectId) {
    runner
        .act(GameAction::ActivateAbility {
            source_id: niv,
            ability_index: 0,
            payment_mode: CastPaymentMode::Auto,
        })
        .expect("Niv-Mizzet's {T} ability must be activatable");
}

/// Resolve until the optional "you may pay that much life" prompt shows up.
fn drive_to_optional(runner: &mut GameRunner, niv: ObjectId) {
    for _ in 0..20 {
        match runner.state().waiting_for.clone() {
            WaitingFor::OptionalEffectChoice { .. } | WaitingFor::OptionalCostChoice { .. } => {
                return;
            }
            WaitingFor::Priority { .. } => {
                if runner.state().stack.is_empty() {
                    break;
                }
                runner.act(GameAction::PassPriority).expect("pass priority");
            }
            other => panic!("unexpected wait: {other:?}"),
        }
    }
    panic!(
        "gaining life must offer Niv-Mizzet's 'you may pay that much life'; waiting {:?}; \
         triggers: {:#?}",
        runner.state().waiting_for,
        runner.state().objects[&niv].trigger_definitions,
    );
}

fn accept(runner: &mut GameRunner, accept: bool) {
    let action = match runner.state().waiting_for {
        WaitingFor::OptionalCostChoice { .. } => GameAction::DecideOptionalCost { pay: accept },
        _ => GameAction::DecideOptionalEffect { accept },
    };
    runner.act(action).expect("answer the optional prompt");
    runner.advance_until_stack_empty();
}

/// CR 119.10 + CR 603.2: gaining life fires "whenever you gain life"; paying
/// that much life (1) draws that many cards (1).
#[test]
fn niv_mizzet_ghost_counsel_pays_life_to_draw_after_gaining_life() {
    let (mut runner, niv) = niv_scenario();
    let life_before = runner.life(P0);
    let opp_before = runner.life(P1);
    let hand_before = runner.state().players[P0.0 as usize].hand.len();

    activate_niv(&mut runner, niv);
    drive_to_optional(&mut runner, niv);
    accept(&mut runner, true);

    assert_eq!(runner.life(P1), opp_before - 1);
    // +1 from the ability, -1 paid to the trigger.
    assert_eq!(runner.life(P0), life_before);
    assert_eq!(
        runner.state().players[P0.0 as usize].hand.len(),
        hand_before + 1,
        "paying 1 life must draw 1 card"
    );
}

#[test]
fn niv_mizzet_ghost_counsel_declining_keeps_life_and_hand() {
    let (mut runner, niv) = niv_scenario();
    let life_before = runner.life(P0);
    let hand_before = runner.state().players[P0.0 as usize].hand.len();

    activate_niv(&mut runner, niv);
    drive_to_optional(&mut runner, niv);
    accept(&mut runner, false);

    assert_eq!(runner.life(P0), life_before + 1);
    assert_eq!(
        runner.state().players[P0.0 as usize].hand.len(),
        hand_before
    );
}
