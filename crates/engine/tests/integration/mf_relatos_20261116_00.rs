//! Relato da mesa de 15/11/2026 (motor v0.125.0) — **Indominus Rex, Alpha**:
//! "As Indominus Rex enters, discard any number of creature cards. It enters
//! with a flying counter on it if a card discarded this way has flying. The same
//! is true for first strike, double strike, deathtouch, hexproof, haste,
//! indestructible, lifelink, menace, reach, trample, and vigilance.
//! When Indominus Rex enters, draw a card for each counter on it."
//!
//! "Não consigo ativar o efeito do Indominus Rex": a primeira linha inteira era
//! uma lacuna (`enters_with_clause`) — o comandante entrava sem perguntar o
//! descarte, sem marcador e sem comprar.

use engine::game::scenario::{GameRunner, GameScenario, P0};
use engine::parser::oracle::parse_oracle_text;
use engine::types::ability::Effect;
use engine::types::actions::GameAction;
use engine::types::counter::CounterType;
use engine::types::game_state::WaitingFor;
use engine::types::identifiers::ObjectId;
use engine::types::keywords::KeywordKind;
use engine::types::mana::{ManaCost, ManaCostShard, ManaType, ManaUnit};
use engine::types::phase::Phase;
use engine::types::replacements::ReplacementEvent;
use engine::types::zones::Zone;

const INDOMINUS: &str = "As Indominus Rex enters, discard any number of creature cards. It \
enters with a flying counter on it if a card discarded this way has flying. The same is true for \
first strike, double strike, deathtouch, hexproof, haste, indestructible, lifelink, menace, \
reach, trample, and vigilance.\nWhen Indominus Rex enters, draw a card for each counter on it.";

#[test]
fn as_enters_discard_line_is_one_replacement_with_a_gated_counter_per_keyword() {
    let parsed = parse_oracle_text(
        INDOMINUS,
        "Indominus Rex, Alpha",
        &[],
        &["Creature".to_string()],
        &[],
    );
    assert!(
        parsed
            .abilities
            .iter()
            .all(|a| !matches!(&*a.effect, Effect::Unimplemented { .. })),
        "the as-enters line is no longer a gap: {:?}",
        parsed.abilities
    );
    let replacement = parsed
        .replacements
        .iter()
        .find(|r| r.event == ReplacementEvent::Moved)
        .expect("the as-enters replacement");
    let discard = replacement.execute.as_deref().expect("execute");
    let Effect::Discard { count, filter, .. } = &*discard.effect else {
        panic!("expected Discard, got {:?}", discard.effect);
    };
    assert!(count.is_up_to(), "\"any number\" is a may-pick-fewer count");
    assert!(filter.is_some(), "only creature cards may be discarded");

    let mut kinds = Vec::new();
    let mut node = discard.sub_ability.as_deref();
    while let Some(def) = node {
        let Effect::PutCounter {
            counter_type: CounterType::Keyword(kind),
            ..
        } = &*def.effect
        else {
            panic!("expected a keyword counter placement, got {:?}", def.effect);
        };
        assert!(
            def.condition.is_some(),
            "{kind:?}: gated on its own keyword"
        );
        kinds.push(*kind);
        node = def.sub_ability.as_deref();
    }
    assert_eq!(
        kinds,
        vec![
            KeywordKind::Flying,
            KeywordKind::FirstStrike,
            KeywordKind::DoubleStrike,
            KeywordKind::Deathtouch,
            KeywordKind::Hexproof,
            KeywordKind::Haste,
            KeywordKind::Indestructible,
            KeywordKind::Lifelink,
            KeywordKind::Menace,
            KeywordKind::Reach,
            KeywordKind::Trample,
            KeywordKind::Vigilance,
        ]
    );
}

/// The frame's other instruction kinds keep their honest gap.
#[test]
fn other_as_enters_instructions_stay_a_gap() {
    let parsed = parse_oracle_text(
        "Flying\nAs this creature enters, remove all counters from all permanents. This creature \
         enters with a +1/+1 counter on it for each counter removed this way.",
        "Thief of Blood",
        &["Flying".to_string()],
        &["Creature".to_string()],
        &[],
    );
    assert!(parsed
        .replacements
        .iter()
        .all(|r| r.event != ReplacementEvent::Moved));
}

struct Table {
    runner: GameRunner,
    rex: ObjectId,
    flyer: ObjectId,
    second_flyer: ObjectId,
    trampler: ObjectId,
    vanilla: ObjectId,
    shock: ObjectId,
}

fn table() -> Table {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.with_mana_pool(
        P0,
        [
            ManaType::Blue,
            ManaType::Blue,
            ManaType::Green,
            ManaType::Green,
            ManaType::Green,
        ]
        .into_iter()
        .map(|mana| ManaUnit::new(mana, ObjectId(0), false, vec![]))
        .collect(),
    );
    for _ in 0..6 {
        scenario.add_spell_to_library_top(P0, "Opt", true);
    }
    let flyer = scenario
        .add_creature_to_hand_from_oracle(P0, "Storm Crow", 1, 2, "Flying")
        .id();
    let second_flyer = scenario
        .add_creature_to_hand_from_oracle(P0, "Wind Drake", 2, 2, "Flying")
        .id();
    let trampler = scenario
        .add_creature_to_hand_from_oracle(P0, "Charging Rhino", 4, 4, "Trample")
        .id();
    let vanilla = scenario
        .add_creature_to_hand(P0, "Grizzly Bears", 2, 2)
        .id();
    let shock = scenario.add_spell_to_hand(P0, "Shock", true).id();
    let rex = {
        let mut card =
            scenario.add_creature_to_hand_from_oracle(P0, "Indominus Rex, Alpha", 6, 6, INDOMINUS);
        card.with_mana_cost(ManaCost::Cost {
            shards: vec![
                ManaCostShard::BlueBlack,
                ManaCostShard::BlueBlack,
                ManaCostShard::Green,
                ManaCostShard::Green,
            ],
            generic: 1,
        });
        card.id()
    };
    Table {
        runner: scenario.build(),
        rex,
        flyer,
        second_flyer,
        trampler,
        vanilla,
        shock,
    }
}

/// Cast Indominus Rex and drive every prompt, discarding `discard` at the
/// as-enters choice. Returns the card lists the discard choice offered.
fn cast_and_settle(t: &mut Table, discard: &[ObjectId]) -> Vec<Vec<ObjectId>> {
    let card_id = t.runner.state().objects[&t.rex].card_id;
    t.runner
        .act(GameAction::CastSpell {
            object_id: t.rex,
            card_id,
            targets: vec![],
            payment_mode: engine::types::game_state::CastPaymentMode::Auto,
        })
        .expect("cast Indominus Rex");
    let mut offered = Vec::new();
    for _ in 0..60 {
        match t.runner.state().waiting_for.clone() {
            WaitingFor::ManaPayment { .. } => {
                t.runner.act(GameAction::PassPriority).expect("pay");
            }
            WaitingFor::DiscardChoice { cards, up_to, .. } => {
                assert!(up_to, "\"any number\": discarding nothing is legal");
                offered.push(cards.clone());
                t.runner
                    .act(GameAction::SelectCards {
                        cards: discard.to_vec(),
                    })
                    .expect("discard the chosen creature cards");
            }
            WaitingFor::Priority { .. } => {
                if t.runner.state().stack.is_empty() {
                    break;
                }
                t.runner.act(GameAction::PassPriority).expect("pass");
            }
            other => panic!("unexpected prompt: {other:?}"),
        }
    }
    offered
}

fn keyword_counters(runner: &GameRunner, id: ObjectId) -> Vec<(KeywordKind, u32)> {
    let mut counters: Vec<(KeywordKind, u32)> = runner.state().objects[&id]
        .counters
        .iter()
        .filter_map(|(counter, n)| match counter {
            CounterType::Keyword(kind) => Some((*kind, *n)),
            _ => None,
        })
        .collect();
    counters.sort_by_key(|(kind, _)| format!("{kind:?}"));
    counters
}

fn hand(runner: &GameRunner) -> Vec<ObjectId> {
    runner
        .state()
        .players
        .iter()
        .find(|p| p.id == P0)
        .unwrap()
        .hand
        .iter()
        .copied()
        .collect()
}

/// The entry runs inside the cast's resolution and its twelve keyword gates
/// resolve as nested siblings. In the unoptimized test profile the all-false
/// walk needs a little more than the 16 MiB the workspace gives test threads
/// (measured: overflows at 16 MiB, fits in 20 MiB), while an opt-level 1 build
/// of the same scenarios fits in 2 MiB — far inside the shipped WASM's 16 MiB.
/// Every runtime scenario therefore runs on its own, deeper thread.
fn on_deep_stack(scenario: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(64 * 1024 * 1024)
        .spawn(scenario)
        .expect("spawn the scenario thread")
        .join()
        .expect("scenario thread");
}

#[test]
fn indominus_offers_only_creature_cards_and_enters_with_their_keywords() {
    on_deep_stack(|| {
        let mut t = table();
        let (flyer, trampler) = (t.flyer, t.trampler);
        let offered = cast_and_settle(&mut t, &[flyer, trampler]);

        assert_eq!(offered.len(), 1, "one discard choice as it enters");
        let mut cards = offered[0].clone();
        cards.sort();
        let mut expected = vec![t.flyer, t.second_flyer, t.trampler, t.vanilla];
        expected.sort();
        assert_eq!(cards, expected, "creature cards only — not the instant");

        let state = t.runner.state();
        assert_eq!(state.objects[&t.rex].zone, Zone::Battlefield);
        assert_eq!(state.objects[&flyer].zone, Zone::Graveyard);
        assert_eq!(state.objects[&trampler].zone, Zone::Graveyard);
        assert_eq!(
            keyword_counters(&t.runner, t.rex),
            vec![(KeywordKind::Flying, 1), (KeywordKind::Trample, 1)]
        );
        // Second flyer, the vanilla creature and the instant stay; two cards drawn.
        let hand = hand(&t.runner);
        assert!(
            hand.contains(&t.second_flyer) && hand.contains(&t.vanilla) && hand.contains(&t.shock)
        );
        assert_eq!(hand.len(), 5, "draws a card for each of the two counters");
    });
}

#[test]
fn indominus_two_discarded_flyers_give_one_flying_counter() {
    on_deep_stack(|| {
        let mut t = table();
        let (flyer, second_flyer) = (t.flyer, t.second_flyer);
        cast_and_settle(&mut t, &[flyer, second_flyer]);

        assert_eq!(
            keyword_counters(&t.runner, t.rex),
            vec![(KeywordKind::Flying, 1)],
            "one counter per kind, however many discarded cards share it"
        );
        assert_eq!(hand(&t.runner).len(), 4, "three kept cards plus one draw");
    });
}

#[test]
fn indominus_discarding_nothing_gives_no_counter_and_no_card() {
    on_deep_stack(|| {
        let mut t = table();
        cast_and_settle(&mut t, &[]);
        assert!(keyword_counters(&t.runner, t.rex).is_empty());
        assert_eq!(hand(&t.runner).len(), 5, "nothing discarded, nothing drawn");
    });
}

#[test]
fn indominus_discarding_a_creature_without_a_listed_keyword_gives_no_counter() {
    on_deep_stack(|| {
        let mut t = table();
        let vanilla = t.vanilla;
        cast_and_settle(&mut t, &[vanilla]);
        assert_eq!(t.runner.state().objects[&vanilla].zone, Zone::Graveyard);
        assert!(keyword_counters(&t.runner, t.rex).is_empty());
        assert_eq!(hand(&t.runner).len(), 4);
    });
}
