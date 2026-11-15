//! Relato da mesa de 15/11/2026 (motor v0.125.0) — **Lluwen, Imperfect
//! Naturalist**: "When Lluwen enters, mill four cards, then you may put a
//! creature or land card from among the milled cards on top of your library."
//!
//! O robô conjurou a Lluwen e o gatilho MIROU o Savai Triome do jogador, no
//! campo, e o pôs no topo do grimório dele: "from among the milled cards" se
//! perdia quando o destino era o topo do grimório, e a instrução virava um
//! `PutAtLibraryPosition` com filtro `Or[Creature, Land]` sobre o campo inteiro.

use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::parser::oracle::parse_oracle_text;
use engine::types::ability::{Effect, LibraryPosition, TargetFilter};
use engine::types::actions::GameAction;
use engine::types::game_state::WaitingFor;
use engine::types::identifiers::ObjectId;
use engine::types::mana::{ManaColor, ManaCost, ManaCostShard, ManaType, ManaUnit};
use engine::types::phase::Phase;
use engine::types::zones::Zone;

const LLUWEN: &str = "When Lluwen enters, mill four cards, then you may put a creature or land \
card from among the milled cards on top of your library.";

#[test]
fn milled_cards_library_top_continuation_reads_the_milled_set() {
    let parsed = parse_oracle_text(
        LLUWEN,
        "Lluwen, Imperfect Naturalist",
        &[],
        &["Creature".to_string()],
        &[],
    );
    let trigger = parsed.triggers.first().expect("the enters trigger");
    let mill = trigger.execute.as_deref().expect("trigger body");
    assert!(
        matches!(&*mill.effect, Effect::Mill { .. }),
        "first instruction mills: {:?}",
        mill.effect
    );
    let put = mill.sub_ability.as_deref().expect("the put-on-top tail");
    let Effect::PutAtLibraryPosition {
        target, position, ..
    } = &*put.effect
    else {
        panic!("expected PutAtLibraryPosition, got {:?}", put.effect);
    };
    assert!(
        matches!(target, TargetFilter::TrackedSetFiltered { .. }),
        "the choice is scoped to the milled cards, got {target:?}"
    );
    assert_eq!(*position, LibraryPosition::Top);
    let Effect::PutAtLibraryPosition { count, .. } = &*put.effect else {
        unreachable!()
    };
    assert!(count.is_up_to(), "\"you may put a … card\" is a may-pick");
}

struct Table {
    runner: GameRunner,
    lluwen: ObjectId,
    milled_land: ObjectId,
    milled_creature: Option<ObjectId>,
    their_land: ObjectId,
    their_creature: ObjectId,
    own_land: ObjectId,
}

/// Lluwen in P0's hand with {G}{G} floating. P0's library, top first: a land,
/// (optionally) a creature, then instants. P1 and P0 each have a land on the
/// battlefield and P1 a creature — the permanents the old parse targeted.
fn table(with_milled_creature: bool) -> Table {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.with_mana_pool(
        P0,
        (0..2)
            .map(|_| ManaUnit::new(ManaType::Green, ObjectId(0), false, vec![]))
            .collect(),
    );
    // Library is built bottom-up: the last card added is the top.
    for _ in 0..3 {
        scenario.add_spell_to_library_top(P0, "Shock", true);
    }
    for _ in 0..2 {
        scenario.add_spell_to_library_top(P0, "Shock", true);
    }
    let milled_creature = if with_milled_creature {
        let id = scenario.add_creature(P0, "Runeclaw Bear", 2, 2).id();
        Some(id)
    } else {
        scenario.add_spell_to_library_top(P0, "Shock", true);
        None
    };
    let milled_land = scenario.add_land_to_library_top(P0, "Mountain").id();
    let their_land = scenario.add_basic_land(P1, ManaColor::White);
    let own_land = scenario.add_basic_land(P0, ManaColor::Green);
    let their_creature = scenario.add_creature(P1, "Hill Giant", 3, 3).id();
    let lluwen = {
        let mut card = scenario.add_creature_to_hand_from_oracle(
            P0,
            "Lluwen, Imperfect Naturalist",
            1,
            3,
            LLUWEN,
        );
        card.with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::BlackGreen, ManaCostShard::BlackGreen],
            generic: 0,
        });
        card.id()
    };
    let mut runner = scenario.build();
    // The milled creature must come from the library: move the battlefield
    // stand-in under the land (second from the top).
    if let Some(id) = milled_creature {
        let state = runner.state_mut();
        state.battlefield.retain(|x| *x != id);
        let obj = state.objects.get_mut(&id).unwrap();
        obj.zone = Zone::Library;
        let library = &mut state
            .players
            .iter_mut()
            .find(|p| p.id == P0)
            .unwrap()
            .library;
        library.insert(1, id);
    }
    Table {
        runner,
        lluwen,
        milled_land,
        milled_creature,
        their_land,
        their_creature,
        own_land,
    }
}

/// Cast Lluwen and drive every prompt: accept the "you may", pick `pick` when a
/// card choice is offered. Returns each card list that was offered.
fn cast_and_settle(t: &mut Table, pick: Option<ObjectId>) -> Vec<Vec<ObjectId>> {
    let card_id = t.runner.state().objects[&t.lluwen].card_id;
    t.runner
        .act(GameAction::CastSpell {
            object_id: t.lluwen,
            card_id,
            targets: vec![],
            payment_mode: engine::types::game_state::CastPaymentMode::Auto,
        })
        .expect("cast Lluwen");
    let mut offered = Vec::new();
    for _ in 0..40 {
        match t.runner.state().waiting_for.clone() {
            WaitingFor::ManaPayment { .. } => {
                t.runner.act(GameAction::PassPriority).expect("pay");
            }
            WaitingFor::OptionalEffectChoice { .. } => {
                t.runner
                    .act(GameAction::DecideOptionalEffect { accept: true })
                    .expect("accept the optional put");
            }
            WaitingFor::EffectZoneChoice { cards, .. } => {
                offered.push(cards.clone());
                t.runner
                    .act(GameAction::SelectCards {
                        cards: pick.into_iter().collect(),
                    })
                    .expect("pick the milled card");
            }
            WaitingFor::Priority { .. } => {
                if t.runner.state().stack.is_empty() {
                    break;
                }
                t.runner.act(GameAction::PassPriority).expect("pass");
            }
            other => panic!(
                "Lluwen's trigger must not ask for anything else (it has no targets): {other:?}"
            ),
        }
    }
    offered
}

fn library_top(runner: &GameRunner) -> ObjectId {
    runner
        .state()
        .players
        .iter()
        .find(|p| p.id == P0)
        .unwrap()
        .library[0]
}

#[test]
fn lluwen_puts_the_only_milled_land_on_top_and_touches_no_permanent() {
    let mut t = table(false);
    let land = t.milled_land;
    let offered = cast_and_settle(&mut t, Some(land));
    assert_eq!(
        offered,
        vec![vec![land]],
        "the single milled land is offered, not forced"
    );

    let state = t.runner.state();
    assert_eq!(state.objects[&t.their_land].zone, Zone::Battlefield);
    assert_eq!(state.objects[&t.own_land].zone, Zone::Battlefield);
    assert_eq!(state.objects[&t.their_creature].zone, Zone::Battlefield);
    assert_eq!(state.objects[&t.lluwen].zone, Zone::Battlefield);
    assert_eq!(
        state.objects[&land].zone,
        Zone::Library,
        "the milled land goes back on top"
    );
    assert_eq!(library_top(&t.runner), land);
    let graveyard = &state.players.iter().find(|p| p.id == P0).unwrap().graveyard;
    assert_eq!(
        graveyard.len(),
        3,
        "the other three milled cards stay milled"
    );
}

#[test]
fn lluwen_offers_only_the_milled_creature_and_land() {
    let mut t = table(true);
    let creature = t.milled_creature.unwrap();
    let offered = cast_and_settle(&mut t, Some(creature));

    assert_eq!(offered.len(), 1, "one choice among the milled cards");
    let mut cards = offered[0].clone();
    cards.sort();
    let mut expected = vec![t.milled_land, creature];
    expected.sort();
    assert_eq!(
        cards, expected,
        "only milled creature/land cards are offered"
    );

    let state = t.runner.state();
    assert_eq!(library_top(&t.runner), creature);
    assert_eq!(state.objects[&t.milled_land].zone, Zone::Graveyard);
    assert_eq!(state.objects[&t.their_land].zone, Zone::Battlefield);
    assert_eq!(state.objects[&t.their_creature].zone, Zone::Battlefield);
}

#[test]
fn lluwen_may_leave_every_milled_card_in_the_graveyard() {
    let mut t = table(false);
    let offered = cast_and_settle(&mut t, None);
    assert_eq!(offered.len(), 1);
    let state = t.runner.state();
    assert_eq!(state.objects[&t.milled_land].zone, Zone::Graveyard);
    let graveyard = &state.players.iter().find(|p| p.id == P0).unwrap().graveyard;
    assert_eq!(graveyard.len(), 4, "all four milled cards stay milled");
    assert_eq!(state.objects[&t.their_land].zone, Zone::Battlefield);
}
