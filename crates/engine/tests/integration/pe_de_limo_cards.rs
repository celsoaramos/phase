//! Three cards from one reported deck that the engine listed as "doesn't play in
//! full", driven through the real pipeline from their verbatim Oracle text.
//!
//! - Grievous Wound: "Whenever enchanted player is dealt damage, they lose half
//!   their life, rounded up." The trigger was `Unknown` (never fired).
//! - Nasty End: "If the sacrificed creature was legendary, draw three cards
//!   instead." The bare supertype predicate didn't parse, so the override was an
//!   `Unimplemented` sibling and a legendary sacrifice drew two.
//! - Tyrannical Pitlord: "As ~ enters, choose another creature you control. The
//!   chosen creature gets +3/+3 and has flying." Neither line parsed.

use engine::game::combat::AttackTarget;
use engine::game::effects::attach::attach_to_player;
use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::types::ability::{ChosenAttribute, TargetRef};
use engine::types::actions::GameAction;
use engine::types::game_state::{CastPaymentMode, WaitingFor};
use engine::types::identifiers::ObjectId;
use engine::types::keywords::Keyword;
use engine::types::mana::{ManaCost, ManaCostShard, ManaType, ManaUnit};
use engine::types::phase::Phase;
use engine::types::player::PlayerId;
use engine::types::zones::Zone;

const GRIEVOUS_WOUND: &str = "Enchant player\n\
Enchanted player can't gain life.\n\
Whenever enchanted player is dealt damage, they lose half their life, rounded up.";

const NASTY_END: &str = "As an additional cost to cast this spell, sacrifice a creature.\n\
Draw two cards. If the sacrificed creature was legendary, draw three cards instead.";

const TYRANNICAL_PITLORD: &str =
    "As Tyrannical Pitlord enters, choose another creature you control.\n\
The chosen creature gets +3/+3 and has flying.";

fn life(r: &GameRunner, p: PlayerId) -> i32 {
    r.state().players[p.0 as usize].life
}

fn hand_size(r: &GameRunner, p: PlayerId) -> usize {
    r.state().players[p.0 as usize].hand.len()
}

fn mana(kind: ManaType, n: usize) -> Vec<ManaUnit> {
    (0..n)
        .map(|_| ManaUnit::new(kind, ObjectId(0), false, vec![]))
        .collect()
}

/// Pass priority, declare `attackers` at P1 and no blocks, answering trigger
/// ordering, until `done` holds.
fn drive_until(r: &mut GameRunner, attackers: &[ObjectId], done: impl Fn(&GameRunner) -> bool) {
    for _ in 0..200 {
        if done(r) {
            return;
        }
        let action = match &r.state().waiting_for {
            WaitingFor::DeclareAttackers { .. } => GameAction::DeclareAttackers {
                attacks: attackers
                    .iter()
                    .map(|id| (*id, AttackTarget::Player(P1)))
                    .collect(),
                bands: vec![],
            },
            WaitingFor::DeclareBlockers { .. } => GameAction::DeclareBlockers {
                assignments: vec![],
            },
            WaitingFor::OrderTriggers { triggers, .. } => GameAction::OrderTriggers {
                order: (0..triggers.len()).collect(),
            },
            _ => GameAction::PassPriority,
        };
        r.act(action).expect("the turn advances");
    }
    panic!("stop condition never reached");
}

fn grievous_wound_on(enchanted: PlayerId) -> (GameRunner, ObjectId) {
    let mut sc = GameScenario::new();
    sc.at_phase(Phase::PreCombatMain);
    for _ in 0..10 {
        sc.add_card_to_library_top(P0, "Plains");
        sc.add_card_to_library_top(P1, "Plains");
    }
    let bear = sc.add_creature(P0, "Grizzly Bears", 2, 2).id();
    let aura = sc
        .add_creature(P0, "Grievous Wound", 0, 0)
        .as_enchantment()
        .with_subtypes(vec!["Aura"])
        .from_oracle_text_with_keywords(&["enchant"], GRIEVOUS_WOUND)
        .id();
    let mut r = sc.build();
    attach_to_player(r.state_mut(), aura, enchanted);
    (r, bear)
}

/// CR 120.3 + CR 303.4e: 2 combat damage takes the enchanted player to 18, then
/// the trigger takes half of 18 (9) — 9 left. "They" is the damaged player, and
/// "their life" is read after the damage.
#[test]
fn grievous_wound_halves_the_enchanted_players_life_when_dealt_damage() {
    let (mut r, bear) = grievous_wound_on(P1);
    drive_until(&mut r, &[bear], |r| {
        r.state().phase == Phase::PostCombatMain && r.state().stack.is_empty()
    });
    assert_eq!(
        life(&r, P1),
        9,
        "20 − 2 combat damage = 18, then half of 18 rounded up = 9"
    );
    assert_eq!(
        life(&r, P0),
        20,
        "the Aura's controller is not the one who loses life"
    );
}

/// Rounding up: 3 damage → 17 → loses 9 (half of 17 rounded up) → 8.
#[test]
fn grievous_wound_rounds_up() {
    let (mut r, bear) = grievous_wound_on(P1);
    r.state_mut().objects.get_mut(&bear).unwrap().power = Some(3);
    r.state_mut().objects.get_mut(&bear).unwrap().base_power = Some(3);
    drive_until(&mut r, &[bear], |r| {
        r.state().phase == Phase::PostCombatMain && r.state().stack.is_empty()
    });
    assert_eq!(
        life(&r, P1),
        8,
        "20 − 3 = 17, then half of 17 rounded up (9) = 8"
    );
}

/// Only the ENCHANTED player triggers it: with the Aura on P0, damage to P1 does
/// nothing extra.
#[test]
fn grievous_wound_ignores_damage_to_other_players() {
    let (mut r, bear) = grievous_wound_on(P0);
    drive_until(&mut r, &[bear], |r| {
        r.state().phase == Phase::PostCombatMain && r.state().stack.is_empty()
    });
    assert_eq!(life(&r, P1), 18, "only the 2 combat damage");
    assert_eq!(life(&r, P0), 20);
}

/// Cast Nasty End sacrificing `victim`, resolve it, and return how many cards
/// were drawn.
fn nasty_end_draws(legendary_victim: bool) -> usize {
    let mut sc = GameScenario::new();
    sc.at_phase(Phase::PreCombatMain);
    for _ in 0..10 {
        sc.add_card_to_library_top(P0, "Plains");
    }
    let mut victim = sc.add_creature(P0, "Victim", 2, 2);
    if legendary_victim {
        victim.as_legendary();
    }
    let victim = victim.id();
    let spell = sc
        .add_spell_to_hand_from_oracle(P0, "Nasty End", true, NASTY_END)
        .with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::Black],
            generic: 1,
        })
        .id();
    sc.with_mana_pool(P0, mana(ManaType::Black, 2));
    let mut r = sc.build();
    let before = hand_size(&r, P0) - 1; // the spell leaves the hand

    let card_id = r.state().objects[&spell].card_id;
    r.act(GameAction::CastSpell {
        object_id: spell,
        card_id,
        targets: vec![],
        payment_mode: CastPaymentMode::Auto,
    })
    .expect("cast Nasty End");
    for _ in 0..40 {
        let action = match r.state().waiting_for.clone() {
            WaitingFor::PayCost { choices, .. } => {
                assert!(
                    choices.contains(&victim),
                    "the creature is a legal sacrifice"
                );
                GameAction::SelectCards {
                    cards: vec![victim],
                }
            }
            WaitingFor::ManaPayment { .. } => GameAction::PassPriority,
            WaitingFor::Priority { .. } if r.state().stack.is_empty() => break,
            _ => GameAction::PassPriority,
        };
        r.act(action).expect("the cast proceeds");
    }
    assert_eq!(
        r.state().objects[&victim].zone,
        Zone::Graveyard,
        "sacrificed"
    );
    hand_size(&r, P0) - before
}

/// CR 117.1 + CR 608.2c: the cost-paid object's LKI is legendary → three cards.
#[test]
fn nasty_end_draws_three_when_the_sacrificed_creature_was_legendary() {
    assert_eq!(nasty_end_draws(true), 3);
}

#[test]
fn nasty_end_draws_two_otherwise() {
    assert_eq!(nasty_end_draws(false), 2);
}

/// Cast Tyrannical Pitlord and choose the other creature P0 controls as it enters.
fn cast_pitlord() -> (GameRunner, ObjectId, ObjectId, ObjectId) {
    let mut sc = GameScenario::new();
    sc.at_phase(Phase::PreCombatMain);
    let ally = sc.add_creature(P0, "Ally", 2, 2).id();
    let foe = sc.add_creature(P1, "Foe", 2, 2).id();
    let pitlord = sc
        .add_creature_to_hand_from_oracle(P0, "Tyrannical Pitlord", 6, 6, TYRANNICAL_PITLORD)
        .with_subtypes(vec!["Demon"])
        .with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::Black],
            generic: 0,
        })
        .id();
    sc.with_mana_pool(P0, mana(ManaType::Black, 1));
    let mut r = sc.build();
    let card_id = r.state().objects[&pitlord].card_id;
    r.act(GameAction::CastSpell {
        object_id: pitlord,
        card_id,
        targets: vec![],
        payment_mode: CastPaymentMode::Auto,
    })
    .expect("cast the Pitlord");
    let mut prompted = false;
    for _ in 0..40 {
        let action = match r.state().waiting_for.clone() {
            WaitingFor::ChooseObjectsSelection { eligible, .. } => {
                prompted = true;
                assert_eq!(
                    eligible,
                    vec![TargetRef::Object(ally)],
                    "only ANOTHER creature YOU control is eligible"
                );
                GameAction::SelectTargets {
                    targets: vec![TargetRef::Object(ally)],
                }
            }
            WaitingFor::ManaPayment { .. } => GameAction::PassPriority,
            WaitingFor::Priority { .. }
                if r.state().stack.is_empty()
                    && r.state().objects[&pitlord].zone == Zone::Battlefield =>
            {
                break;
            }
            _ => GameAction::PassPriority,
        };
        r.act(action).expect("the cast proceeds");
    }
    assert!(prompted, "the as-enters choice must be asked");
    (r, pitlord, ally, foe)
}

fn pt(r: &GameRunner, id: ObjectId) -> (i32, i32) {
    let o = &r.state().objects[&id];
    (o.power.unwrap_or(0), o.toughness.unwrap_or(0))
}

/// CR 614.12a + CR 607.2d + CR 613.1: the chosen creature gets +3/+3 and flying
/// while the Pitlord is on the battlefield; nothing else does.
#[test]
fn tyrannical_pitlord_pumps_the_chosen_creature() {
    let (mut r, pitlord, ally, foe) = cast_pitlord();
    engine::game::layers::evaluate_layers(r.state_mut());
    assert!(
        r.state().objects[&pitlord]
            .chosen_attributes
            .iter()
            .any(|a| matches!(a, ChosenAttribute::Card(pin) if pin.object_id == ally)),
        "the choice is remembered on the Pitlord"
    );
    assert_eq!(pt(&r, ally), (5, 5), "the chosen creature gets +3/+3");
    assert!(
        r.state().objects[&ally].has_keyword(&Keyword::Flying),
        "the chosen creature has flying"
    );
    assert_eq!(pt(&r, foe), (2, 2), "an opponent's creature is untouched");
    assert_eq!(pt(&r, pitlord), (6, 6), "the Pitlord does not pump itself");
}
