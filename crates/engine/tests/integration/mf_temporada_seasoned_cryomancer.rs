//! Temporada 2026-12 da Mesa do MagicFinder — Dimir Reanimator (Standard) recusado
//! porque Seasoned Cryomancer saía com `zone_change_reflexive_unbound_referent`:
//! "When this creature enters, draw two cards, then discard two cards. When you
//! discard one or more nonland cards this way, tap up to that many target creatures
//! and put a stun counter on each of them."
//!
//! A forma com a segunda instrução em frase separada já era suportada. No
//! composto com " and ", o pronome da CONTINUAÇÃO ("each of them") fazia o braço
//! condicionado reescrever o alvo da CABEÇA ("up to that many target creatures")
//! para o referente do pai — e o guarda do reflexivo recusava o resultado. A
//! cabeça agora mantém o próprio alvo (CR 608.2c + CR 115.1).

use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::parser::oracle::parse_oracle_text;
use engine::types::ability::{Effect, TargetFilter, TargetRef};
use engine::types::actions::GameAction;
use engine::types::counter::CounterType;
use engine::types::game_state::WaitingFor;
use engine::types::identifiers::ObjectId;
use engine::types::mana::{ManaCost, ManaCostShard, ManaType, ManaUnit};
use engine::types::phase::Phase;

const CRYOMANCER: &str = "When this creature enters, draw two cards, then discard two cards. \
When you discard one or more nonland cards this way, tap up to that many target creatures and \
put a stun counter on each of them.\n{3}{U}{U}, Exile this card from your graveyard: Draw two \
cards.";

fn unimplemented(def: &engine::types::ability::AbilityDefinition) -> bool {
    let mut node = Some(def);
    while let Some(d) = node {
        if matches!(&*d.effect, Effect::Unimplemented { .. }) {
            return true;
        }
        node = d.sub_ability.as_deref();
    }
    false
}

#[test]
fn the_reflexive_keeps_its_own_target_and_stuns_each_of_them() {
    let parsed = parse_oracle_text(
        CRYOMANCER,
        "Seasoned Cryomancer",
        &[],
        &["Creature".to_string()],
        &["Human".to_string(), "Wizard".to_string()],
    );
    let etb = parsed.triggers[0].execute.as_deref().expect("ETB body");
    assert!(!unimplemented(etb), "{etb:?}");
    let tap = etb
        .sub_ability
        .as_deref()
        .and_then(|discard| discard.sub_ability.as_deref())
        .expect("the reflexive node");
    let Effect::SetTapState { target, .. } = &*tap.effect else {
        panic!("expected the tap, got {:?}", tap.effect);
    };
    assert!(
        matches!(target, TargetFilter::Typed(_)),
        "\"up to that many target creatures\" is the reflexive's own target: {target:?}"
    );
    assert!(tap.multi_target.is_some(), "up to that many");
    let stun = tap.sub_ability.as_deref().expect("the stun conjunct");
    assert!(matches!(
        &*stun.effect,
        Effect::PutCounter {
            counter_type: CounterType::Stun,
            target: TargetFilter::ParentTarget,
            ..
        }
    ));
}

struct Board {
    runner: GameRunner,
    cryo: ObjectId,
    discards: Vec<ObjectId>,
    bears: Vec<ObjectId>,
}

/// Cryomancer in hand (castable with the pool), the cards to discard already in
/// hand (`nonland` instants + `lands` Islands), two cards on top of the library,
/// three opposing creatures.
fn board(nonland: usize, lands: usize) -> Board {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.with_mana_pool(
        P0,
        [ManaType::Blue, ManaType::Blue, ManaType::Colorless]
            .into_iter()
            .map(|c| ManaUnit::new(c, ObjectId(0), false, vec![]))
            .collect(),
    );
    let cryo = {
        let mut b =
            scenario.add_creature_to_hand_from_oracle(P0, "Seasoned Cryomancer", 3, 3, CRYOMANCER);
        b.with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::Blue, ManaCostShard::Blue],
            generic: 1,
        });
        b.id()
    };
    let mut discards = Vec::new();
    for i in 0..nonland {
        discards.push(
            scenario
                .add_spell_to_hand(P0, &format!("Spell {i}"), true)
                .id(),
        );
    }
    for i in 0..lands {
        discards.push(scenario.add_land_to_hand(P0, &format!("Island {i}")).id());
    }
    scenario.add_card_to_library_top(P0, "Drawn A");
    scenario.add_card_to_library_top(P0, "Drawn B");
    let bears: Vec<ObjectId> = (0..3)
        .map(|i| scenario.add_creature(P1, &format!("Bear {i}"), 2, 2).id())
        .collect();
    Board {
        runner: scenario.build(),
        cryo,
        discards,
        bears,
    }
}

/// Cast Cryomancer, discard exactly `discards`, and answer every target slot with
/// the next untapped bear. Returns how many target slots were offered.
fn play(board: &mut Board) -> usize {
    let Board {
        runner,
        cryo,
        discards,
        bears,
    } = board;
    let card_id = runner.state().objects[cryo].card_id;
    runner
        .act(GameAction::CastSpell {
            object_id: *cryo,
            card_id,
            targets: vec![],
            payment_mode: engine::types::game_state::CastPaymentMode::Auto,
        })
        .expect("Cryomancer is castable");
    let mut slots = 0;
    let mut chosen: Vec<TargetRef> = Vec::new();
    for _ in 0..40 {
        match runner.state().waiting_for.clone() {
            WaitingFor::DiscardChoice { .. } => {
                runner
                    .act(GameAction::SelectCards {
                        cards: discards.clone(),
                    })
                    .expect("discard the chosen two");
            }
            WaitingFor::TriggerTargetSelection {
                target_slots,
                selection,
                ..
            } => {
                slots = slots.max(selection.current_slot + 1);
                let legal = &target_slots[selection.current_slot].legal_targets;
                let pick = bears
                    .iter()
                    .map(|b| TargetRef::Object(*b))
                    .find(|t| legal.contains(t) && !chosen.contains(t));
                if let Some(p) = &pick {
                    chosen.push(p.clone());
                }
                runner
                    .act(GameAction::ChooseTarget { target: pick })
                    .expect("target accepted");
            }
            WaitingFor::Priority { .. } => {
                if runner.state().stack.is_empty() {
                    break;
                }
                runner.act(GameAction::PassPriority).expect("pass");
            }
            other => panic!("unexpected state: {other:?}"),
        }
    }
    chosen.len().max(slots)
}

fn stunned_and_tapped(runner: &GameRunner, bears: &[ObjectId]) -> usize {
    bears
        .iter()
        .filter(|b| {
            let obj = &runner.state().objects[*b];
            obj.tapped && obj.counters.get(&CounterType::Stun).copied().unwrap_or(0) == 1
        })
        .count()
}

#[test]
fn two_nonland_discards_tap_and_stun_up_to_two_creatures() {
    let mut b = board(2, 0);
    play(&mut b);
    assert_eq!(stunned_and_tapped(&b.runner, &b.bears), 2);
}

#[test]
fn a_land_and_a_nonland_discard_stun_up_to_one_creature() {
    let mut b = board(1, 1);
    play(&mut b);
    assert_eq!(stunned_and_tapped(&b.runner, &b.bears), 1);
}

#[test]
fn two_land_discards_create_no_reflexive_trigger() {
    let mut b = board(0, 2);
    let offered = play(&mut b);
    assert_eq!(offered, 0, "no nonland card was discarded this way");
    assert_eq!(stunned_and_tapped(&b.runner, &b.bears), 0);
    assert!(b
        .bears
        .iter()
        .all(|id| !b.runner.state().objects[id].tapped));
}

/// The same compound behind a "you may pay {2}. When you do, …" reflexive
/// (Snaremaster Sprite): the head keeps "target creature an opponent controls"
/// instead of being rebound to the (target-less) payment.
#[test]
fn snaremaster_sprite_taps_and_stuns_the_chosen_opposing_creature() {
    const SNAREMASTER: &str = "Flying\nWhen this creature enters, you may pay {2}. When you \
do, tap target creature an opponent controls and put a stun counter on it.";
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.with_mana_pool(
        P0,
        (0..5)
            .map(|_| ManaUnit::new(ManaType::Blue, ObjectId(0), false, vec![]))
            .collect(),
    );
    let sprite = {
        let mut b =
            scenario.add_creature_to_hand_from_oracle(P0, "Snaremaster Sprite", 2, 1, SNAREMASTER);
        b.with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::Blue],
            generic: 2,
        });
        b.id()
    };
    let mine = scenario.add_creature(P0, "My Bear", 2, 2).id();
    let theirs = scenario.add_creature(P1, "Their Bear", 2, 2).id();
    let mut runner = scenario.build();
    let card_id = runner.state().objects[&sprite].card_id;
    runner
        .act(GameAction::CastSpell {
            object_id: sprite,
            card_id,
            targets: vec![],
            payment_mode: engine::types::game_state::CastPaymentMode::Auto,
        })
        .expect("castable");
    let mut offered_mine = false;
    for _ in 0..40 {
        match runner.state().waiting_for.clone() {
            WaitingFor::OptionalCostChoice { .. } => {
                runner
                    .act(GameAction::DecideOptionalCost { pay: true })
                    .expect("pay {2}");
            }
            WaitingFor::OptionalEffectChoice { .. } => {
                runner
                    .act(GameAction::DecideOptionalEffect { accept: true })
                    .expect("pay {2}");
            }
            WaitingFor::TriggerTargetSelection {
                target_slots,
                selection,
                ..
            } => {
                let legal = &target_slots[selection.current_slot].legal_targets;
                offered_mine |= legal.contains(&TargetRef::Object(mine));
                runner
                    .act(GameAction::ChooseTarget {
                        target: Some(TargetRef::Object(theirs)),
                    })
                    .expect("their creature is a legal target");
            }
            WaitingFor::Priority { .. } => {
                if runner.state().stack.is_empty() {
                    break;
                }
                runner.act(GameAction::PassPriority).expect("pass");
            }
            other => panic!("unexpected state: {other:?}"),
        }
    }
    assert!(!offered_mine, "only a creature an opponent controls");
    let obj = &runner.state().objects[&theirs];
    assert!(obj.tapped);
    assert_eq!(obj.counters.get(&CounterType::Stun).copied(), Some(1));
    assert!(!runner.state().objects[&mine].tapped);
}
