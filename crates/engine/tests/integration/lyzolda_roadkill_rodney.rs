//! Runtime regressions for two field reports (Magic Finder, 2026-09-28).
//!
//! Lyzolda, the Blood Witch — "{2}, Sacrifice a creature: Lyzolda deals 2
//! damage to any target if the sacrificed creature was red. Draw a card if the
//! sacrificed creature was black." The two sentences are independent
//! instructions (CR 608.2c), each gated on the cost-paid object (CR 608.2k).
//! Sacrificing a mono-black creature must still draw even though the damage
//! clause's "was red" gate is false.
//!
//! Roadkill Rodney — "Squad {3}". CR 601.2f + CR 702.157a: another squad
//! payment is only offered while the total stays payable. With {2} + {3} and
//! five lands, accepting a second payment used to leave only an unpayable
//! "pay" and Cancel — the cast could not finish.

use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::types::ability::AbilityKind;
use engine::types::actions::GameAction;
use engine::types::game_state::{CastPaymentMode, WaitingFor};
use engine::types::identifiers::ObjectId;
use engine::types::mana::{ManaColor, ManaCost, ManaCostShard, ManaType, ManaUnit};
use engine::types::phase::Phase;

const LYZOLDA: &str = "{2}, Sacrifice a creature: Lyzolda deals 2 damage to any target if the sacrificed creature was red. Draw a card if the sacrificed creature was black.";
const RODNEY: &str = "Squad {3} (As an additional cost to cast this spell, you may pay {3} any number of times. When this creature enters, create that many tokens that are copies of it.)\nDeathtouch\nWhenever this creature deals combat damage to a player, create a Mutagen token.";

fn colorless(n: usize) -> Vec<ManaUnit> {
    (0..n)
        .map(|_| ManaUnit::new(ManaType::Colorless, ObjectId(0), false, vec![]))
        .collect()
}

fn life(r: &GameRunner, player: engine::types::player::PlayerId) -> i32 {
    r.state()
        .players
        .iter()
        .find(|p| p.id == player)
        .unwrap()
        .life
}

fn hand(r: &GameRunner) -> usize {
    r.state()
        .players
        .iter()
        .find(|p| p.id == P0)
        .unwrap()
        .hand
        .len()
}

/// Activate Lyzolda sacrificing a creature of `colors`; returns (damage to P1, cards drawn).
fn lyzolda_sacrificing(colors: &[ManaColor]) -> (i32, usize) {
    let mut sc = GameScenario::new();
    sc.at_phase(Phase::PreCombatMain);
    sc.with_mana_pool(P0, colorless(2));
    sc.with_library_top(P0, &["Card A", "Card B"]);
    let lyz = sc
        .add_creature_from_oracle(P0, "Lyzolda, the Blood Witch", 3, 1, LYZOLDA)
        .id();
    let fodder = {
        let mut b = sc.add_creature(P0, "Fodder", 1, 1);
        b.with_color(colors.to_vec());
        b.with_mana_cost(ManaCost::Cost {
            shards: colors
                .iter()
                .map(|c| match c {
                    ManaColor::Black => ManaCostShard::Black,
                    _ => ManaCostShard::Red,
                })
                .collect(),
            generic: 0,
        });
        b.id()
    };
    let mut r = sc.build();
    let idx = r.state().objects[&lyz]
        .abilities
        .iter()
        .position(|a| matches!(a.kind, AbilityKind::Activated))
        .expect("Lyzolda has an activated ability");
    let (life0, hand0) = (life(&r, P1), hand(&r));
    r.activate(lyz, idx)
        .target_player(P1)
        .pay_with(&[fodder])
        .resolve();
    (life0 - life(&r, P1), hand(&r) - hand0)
}

#[test]
fn lyzolda_black_and_red_sacrifice_deals_damage_and_draws() {
    assert_eq!(
        lyzolda_sacrificing(&[ManaColor::Black, ManaColor::Red]),
        (2, 1)
    );
}

#[test]
fn lyzolda_red_sacrifice_only_deals_damage() {
    assert_eq!(lyzolda_sacrificing(&[ManaColor::Red]), (2, 0));
}

#[test]
fn lyzolda_black_sacrifice_still_draws_when_damage_gate_is_false() {
    assert_eq!(
        lyzolda_sacrificing(&[ManaColor::Black]),
        (0, 1),
        "the draw sentence is its own instruction; a false 'was red' gate must not skip it"
    );
}

/// Cast Rodney with `lands` Swamps, answering "pay" to every squad prompt.
/// Returns (Rodneys on the battlefield, squad prompts seen).
fn rodney_paying_every_squad(lands: usize) -> (usize, usize) {
    let mut sc = GameScenario::new();
    sc.at_phase(Phase::PreCombatMain);
    for _ in 0..lands {
        sc.add_basic_land(P0, ManaColor::Black);
    }
    let spell = {
        let mut b = sc.add_creature_to_hand(P0, "Roadkill Rodney", 2, 1);
        b.as_artifact();
        b.with_mana_cost(ManaCost::generic(2));
        b.from_oracle_text_with_keywords(&["Squad", "Deathtouch"], RODNEY);
        b.id()
    };
    let mut r = sc.build();
    let card_id = r.state().objects[&spell].card_id;
    r.act(GameAction::CastSpell {
        object_id: spell,
        card_id,
        targets: vec![],
        payment_mode: CastPaymentMode::Auto,
    })
    .expect("Rodney is castable");
    let mut prompts = 0;
    for _ in 0..16 {
        match r.state().waiting_for.clone() {
            WaitingFor::OptionalCostChoice { .. } => {
                prompts += 1;
                r.act(GameAction::DecideOptionalCost { pay: true })
                    .expect("an offered squad payment must be payable");
            }
            WaitingFor::Priority { .. } => break,
            _ => {
                r.act(GameAction::PassPriority)
                    .expect("payment step advances");
            }
        }
    }
    r.advance_until_stack_empty();
    let rodneys = r
        .battlefield_names()
        .iter()
        .filter(|n| *n == "Roadkill Rodney")
        .count();
    (rodneys, prompts)
}

#[test]
fn rodney_squad_is_only_offered_while_payable() {
    // {2} + one Squad {3} = 5: exactly one squad payment is affordable.
    assert_eq!(rodney_paying_every_squad(5), (2, 1));
    // {2} + two Squad {3} = 8.
    assert_eq!(rodney_paying_every_squad(8), (3, 2));
    // Only the base cost: no squad prompt at all.
    assert_eq!(rodney_paying_every_squad(2), (1, 0));
}
