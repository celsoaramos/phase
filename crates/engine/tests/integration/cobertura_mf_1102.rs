//! Terceiro lote de cobertura (medição de 30/10/2026, build `v0.119.0-mf`).
//! Cada teste passa pelo pipeline real de conjurar/resolver.
//!
//! - Tibalt's Trickery (CR 701.6a + CR 701.13a + CR 601.2 + CR 109.5):
//!   "Choose 1, 2, or 3 at random. Its controller mills that many cards, then
//!   exiles cards from the top of their library until they exile a nonland card
//!   with a different name than that spell. They may cast that card without
//!   paying its mana cost." — o número escolhido, o "different name", e quem
//!   conjura (o controlador da mágica anulada, não o de Tibalt).
//! - Arwen, Mortal Queen (CR 122.1 + CR 608.2c): "Put a +1/+1 counter and a
//!   lifelink counter on that creature and a +1/+1 counter and a lifelink
//!   counter on Arwen" — a lista de marcadores com um segundo destinatário.

use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::types::actions::GameAction;
use engine::types::card_type::CoreType;
use engine::types::counter::{parse_counter_type, CounterType};
use engine::types::game_state::{CastingVariant, StackEntry, StackEntryKind, WaitingFor};
use engine::types::identifiers::{CardId, ObjectId};
use engine::types::keywords::Keyword;
use engine::types::mana::{ManaColor, ManaCost, ManaCostShard};
use engine::types::phase::Phase;
use engine::types::player::PlayerId;
use engine::types::zones::Zone;

const TIBALTS_TRICKERY: &str = "Counter target spell. Choose 1, 2, or 3 at random. Its \
    controller mills that many cards, then exiles cards from the top of their library until they \
    exile a nonland card with a different name than that spell. They may cast that card without \
    paying its mana cost. Then they put the exiled cards on the bottom of their library in a \
    random order.";

/// Put a P1 instant named "Shock" on the stack, as if P1 had just cast it.
fn put_shock_on_stack(runner: &mut GameRunner) -> ObjectId {
    let spell = engine::game::zones::create_object(
        runner.state_mut(),
        CardId(701),
        P1,
        "Shock".to_string(),
        Zone::Stack,
    );
    runner
        .state_mut()
        .objects
        .get_mut(&spell)
        .unwrap()
        .card_types
        .core_types = vec![CoreType::Instant];
    runner.state_mut().stack.push_back(StackEntry {
        id: spell,
        source_id: spell,
        controller: P1,
        kind: StackEntryKind::Spell {
            card_id: CardId(701),
            ability: None,
            casting_variant: CastingVariant::Normal,
            actual_mana_spent: 0,
        },
    });
    spell
}

struct Trickery {
    runner: GameRunner,
    mountains: Vec<ObjectId>,
    other_shock: ObjectId,
    windfall: ObjectId,
    filler: ObjectId,
}

/// P1's library, top to bottom: three Mountains (the mill reaches 1–3 of
/// them), another "Shock" (same name as the countered spell: skipped), a
/// "You gain 5 life" sorcery (the hit) and one land never reached.
fn tibalt_to_cast_prompt() -> Trickery {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let tibalt = scenario
        .add_spell_to_hand_from_oracle(P0, "Tibalt's Trickery", true, TIBALTS_TRICKERY)
        .with_mana_cost(ManaCost::Cost {
            generic: 1,
            shards: vec![ManaCostShard::Red],
        })
        .id();
    scenario.add_basic_land(P0, ManaColor::Red);
    scenario.add_basic_land(P0, ManaColor::Red);

    let filler = scenario
        .add_land_to_library_top(P1, "Unreached Plains")
        .id();
    let windfall = scenario
        .add_spell_to_library_top(P1, "Windfall Gift", false)
        .from_oracle_text("You gain 5 life.")
        .with_mana_cost(ManaCost::Cost {
            generic: 7,
            shards: vec![],
        })
        .id();
    let other_shock = scenario.add_spell_to_library_top(P1, "Shock", true).id();
    let mountains = (0..3)
        .map(|i| {
            scenario
                .add_land_to_library_top(P1, &format!("Mountain {i}"))
                .id()
        })
        .collect();
    let mut runner = scenario.build();
    let shock = put_shock_on_stack(&mut runner);

    runner.cast(tibalt).target_objects(&[shock]).commit();
    for _ in 0..16 {
        match runner.state().waiting_for.clone() {
            WaitingFor::OptionalEffectChoice { .. } => break,
            WaitingFor::Priority { .. } => {
                runner.act(GameAction::PassPriority).expect("pass priority");
            }
            other => panic!("unexpected state while resolving Tibalt's Trickery: {other:?}"),
        }
    }
    assert_eq!(
        runner.state().objects[&shock].zone,
        Zone::Graveyard,
        "the target spell is countered"
    );
    Trickery {
        runner,
        mountains,
        other_shock,
        windfall,
        filler,
    }
}

fn zone(runner: &GameRunner, id: ObjectId) -> Zone {
    runner.state().objects[&id].zone
}

fn life(runner: &GameRunner, player: PlayerId) -> i32 {
    runner.state().players[player.0 as usize].life
}

/// The milled Mountains are 1–3 (chosen at random); the rest of the reveal
/// skips lands and the same-named Shock and stops on the sorcery.
fn assert_reveal(t: &Trickery) {
    let milled = t
        .mountains
        .iter()
        .filter(|&&id| zone(&t.runner, id) == Zone::Graveyard)
        .count();
    assert!(
        (1..=3).contains(&milled),
        "its controller mills 1, 2 or 3 cards, got {milled}"
    );
    assert!(
        t.mountains
            .iter()
            .all(|&id| matches!(zone(&t.runner, id), Zone::Graveyard | Zone::Exile)),
        "the Mountains not milled are exiled by the reveal"
    );
    assert_eq!(
        zone(&t.runner, t.other_shock),
        Zone::Exile,
        "a card with the countered spell's name does not stop the reveal"
    );
    assert_eq!(
        zone(&t.runner, t.windfall),
        Zone::Exile,
        "the reveal stops here"
    );
    assert_eq!(zone(&t.runner, t.filler), Zone::Library);
}

#[test]
fn tibalts_trickery_offers_the_free_cast_to_the_countered_spells_controller() {
    let mut t = tibalt_to_cast_prompt();
    assert_reveal(&t);
    let WaitingFor::OptionalEffectChoice { player, .. } = t.runner.state().waiting_for.clone()
    else {
        unreachable!()
    };
    assert_eq!(
        player, P1,
        "\"they may cast\" asks the countered spell's controller"
    );

    t.runner
        .act(GameAction::DecideOptionalEffect { accept: true })
        .expect("accept the free cast");
    for _ in 0..16 {
        if !t.runner.state().stack.is_empty() {
            t.runner
                .act(GameAction::PassPriority)
                .expect("pass priority");
        }
    }

    assert_eq!(life(&t.runner, P1), 25, "P1 cast the sorcery and gained 5");
    assert_eq!(
        life(&t.runner, P0),
        20,
        "Tibalt's controller did not cast it"
    );
    assert_eq!(zone(&t.runner, t.windfall), Zone::Graveyard);
    let library = &t.runner.state().players[P1.0 as usize].library;
    assert_eq!(library.front().copied(), Some(t.filler));
    assert!(
        library.contains(&t.other_shock),
        "the exiled cards that weren't cast go to the bottom of their library"
    );
    assert!(
        t.mountains
            .iter()
            .all(|&id| zone(&t.runner, id) != Zone::Exile),
        "no exiled Mountain is left in exile"
    );
}

#[test]
fn tibalts_trickery_declined_cast_bottoms_every_exiled_card() {
    let mut t = tibalt_to_cast_prompt();
    t.runner
        .act(GameAction::DecideOptionalEffect { accept: false })
        .expect("decline the free cast");
    for _ in 0..16 {
        if !t.runner.state().stack.is_empty() {
            t.runner
                .act(GameAction::PassPriority)
                .expect("pass priority");
        }
    }
    assert_eq!(life(&t.runner, P1), 20, "nothing was cast");
    let library = &t.runner.state().players[P1.0 as usize].library;
    assert_eq!(library.front().copied(), Some(t.filler));
    assert!(library.contains(&t.windfall) && library.contains(&t.other_shock));
    assert_eq!(
        t.runner.state().exile.len(),
        0,
        "every exiled card went to the bottom"
    );
}

const ARWEN: &str = "Arwen enters with an indestructible counter on her.\n{1}, Remove an \
    indestructible counter from Arwen: Another target creature gains indestructible until end of \
    turn. Put a +1/+1 counter and a lifelink counter on that creature and a +1/+1 counter and a \
    lifelink counter on Arwen.";

fn counters(runner: &GameRunner, id: ObjectId, kind: &CounterType) -> u32 {
    runner.state().objects[&id]
        .counters
        .get(kind)
        .copied()
        .unwrap_or(0)
}

#[test]
fn arwen_puts_both_counters_on_the_target_and_on_herself() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let arwen = scenario
        .add_creature(P0, "Arwen", 2, 2)
        .from_oracle_text(ARWEN)
        .id();
    let indestructible = parse_counter_type("indestructible");
    scenario.with_counter(arwen, indestructible.clone(), 1);
    let bear = scenario.add_creature(P0, "Ally Bear", 2, 2).id();
    scenario.add_basic_land(P0, ManaColor::White);
    let mut runner = scenario.build();

    runner.activate(arwen, 0).target_object(bear).resolve();

    let lifelink = parse_counter_type("lifelink");
    assert_eq!(
        counters(&runner, arwen, &indestructible),
        0,
        "the cost removed it"
    );
    assert!(
        runner.state().objects[&bear].has_keyword(&Keyword::Indestructible),
        "the target gains indestructible"
    );
    for (who, id) in [("target", bear), ("Arwen", arwen)] {
        assert_eq!(
            counters(&runner, id, &CounterType::Plus1Plus1),
            1,
            "+1/+1 counter on {who}"
        );
        assert_eq!(
            counters(&runner, id, &lifelink),
            1,
            "lifelink counter on {who}"
        );
    }
}
