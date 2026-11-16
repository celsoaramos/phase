//! Temporada 2026-12 da Mesa do MagicFinder — Uldaros Theorix (Dimir Reanimator,
//! Standard) e The Borg Queen (Commander).
//!
//! Uldaros: "When Uldaros Theorix enters, if you cast him, exile up to one target
//! nonland card of each card type from your graveyard. Copy those cards. You may
//! cast any number of spells with total mana value 6 or less from among the copies
//! without paying their mana costs." — "if you cast him" não era lido (só "it"), "of
//! each card type" sumia (e levava junto o "from your graveyard" do filtro) e o
//! "total mana value 6 or less" não existia no `CastCopyOfCard`.
//!
//! Borg Queen: "… target opponent exiles cards from the top of their library until
//! they exile a creature card. Assimilate that card. They put the rest on the
//! bottom in a random order." — "Assimilate" só aceitava "target … from a
//! graveyard", e "the rest on the bottom" sem "of their library" não era lido.

use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::parser::oracle::parse_oracle_text;
use engine::types::ability::{
    AbilityDefinition, ChooseFromZoneConstraint, Effect, TargetFilter, TargetRef,
};
use engine::types::actions::GameAction;
use engine::types::game_state::{CastPaymentMode, TargetSelectionConstraint, WaitingFor};
use engine::types::identifiers::ObjectId;
use engine::types::mana::{ManaCost, ManaCostShard, ManaType, ManaUnit};
use engine::types::phase::Phase;
use engine::types::zones::Zone;

const ULDAROS: &str = "Flying\nWhen Uldaros Theorix enters, if you cast him, exile up to one \
target nonland card of each card type from your graveyard. Copy those cards. You may cast any \
number of spells with total mana value 6 or less from among the copies without paying their \
mana costs. (Permanent spells cast this way become tokens.)";

const BORG_QUEEN: &str = "Artifact creatures you control get +2/+2.\nWhenever The Borg Queen \
enters or attacks, target opponent exiles cards from the top of their library until they exile \
a creature card. Assimilate that card. They put the rest on the bottom in a random order.";

fn chain(def: &AbilityDefinition) -> Vec<&AbilityDefinition> {
    let mut out = Vec::new();
    let mut node = Some(def);
    while let Some(d) = node {
        out.push(d);
        node = d.sub_ability.as_deref();
    }
    out
}

#[test]
fn uldaros_parses_cast_gate_per_type_targets_and_bounded_copies() {
    let parsed = parse_oracle_text(
        ULDAROS,
        "Uldaros Theorix",
        &["Flying".to_string()],
        &["Creature".to_string()],
        &["Elder".to_string(), "Sphinx".to_string()],
    );
    let trigger = &parsed.triggers[0];
    assert!(
        trigger.condition.is_some(),
        "\"if you cast him\" is the intervening if"
    );
    let nodes = chain(trigger.execute.as_deref().unwrap());
    assert!(
        nodes
            .iter()
            .all(|d| !matches!(&*d.effect, Effect::Unimplemented { .. })),
        "{nodes:?}"
    );
    let exile = nodes[0];
    let Effect::ChangeZone { target, .. } = &*exile.effect else {
        panic!("expected the exile, got {:?}", exile.effect);
    };
    assert!(
        format!("{target:?}").contains("Graveyard"),
        "\"from your graveyard\" survives the per-type qualifier: {target:?}"
    );
    assert!(exile
        .target_constraints
        .contains(&TargetSelectionConstraint::DistinctCardTypes));
    assert!(nodes.iter().any(|d| matches!(
        &*d.effect,
        Effect::CastCopyOfCard {
            constraint: Some(ChooseFromZoneConstraint::TotalManaValue { value: 6, .. }),
            ..
        }
    )));
}

#[test]
fn borg_queen_assimilates_the_found_card_and_bottoms_the_rest() {
    let parsed = parse_oracle_text(
        BORG_QUEEN,
        "The Borg Queen",
        &[],
        &["Artifact".to_string(), "Creature".to_string()],
        &["Borg".to_string(), "Noble".to_string()],
    );
    let nodes = chain(parsed.triggers[0].execute.as_deref().unwrap());
    assert!(
        nodes
            .iter()
            .all(|d| !matches!(&*d.effect, Effect::Unimplemented { .. })),
        "{nodes:?}"
    );
    assert!(nodes.iter().any(|d| matches!(
        &*d.effect,
        Effect::ChangeZone {
            destination: Zone::Battlefield,
            target: TargetFilter::ParentTarget,
            ..
        }
    )));
    assert!(nodes
        .iter()
        .any(|d| matches!(&*d.effect, Effect::PutAtLibraryPosition { .. })));
}

fn mana(colors: &[ManaType]) -> Vec<ManaUnit> {
    colors
        .iter()
        .map(|c| ManaUnit::new(*c, ObjectId(0), false, vec![]))
        .collect()
}

struct UldarosBoard {
    runner: GameRunner,
    uldaros: ObjectId,
    instant: ObjectId,
    second_instant: ObjectId,
    sorcery: ObjectId,
    bear: ObjectId,
}

fn uldaros_board() -> UldarosBoard {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.with_mana_pool(
        P0,
        mana(&[
            ManaType::Blue,
            ManaType::Black,
            ManaType::Black,
            ManaType::Colorless,
            ManaType::Colorless,
            ManaType::Colorless,
        ]),
    );
    let uldaros = {
        let mut b = scenario.add_creature_to_hand_from_oracle(P0, "Uldaros Theorix", 5, 5, ULDAROS);
        b.with_mana_cost(ManaCost::Cost {
            shards: vec![
                ManaCostShard::Blue,
                ManaCostShard::Black,
                ManaCostShard::Black,
            ],
            generic: 3,
        });
        b.id()
    };
    let instant = {
        let mut b = scenario.add_spell_to_graveyard(P0, "Quick Study", true);
        b.from_oracle_text("Draw a card.")
            .with_mana_cost(ManaCost::generic(2));
        b.id()
    };
    let second_instant = {
        let mut b = scenario.add_spell_to_graveyard(P0, "Second Study", true);
        b.from_oracle_text("Draw a card.")
            .with_mana_cost(ManaCost::generic(1));
        b.id()
    };
    let sorcery = {
        let mut b = scenario.add_spell_to_graveyard(P0, "Big Feast", false);
        b.from_oracle_text("You gain 3 life.")
            .with_mana_cost(ManaCost::generic(3));
        b.id()
    };
    let bear = scenario
        .add_creature_to_graveyard(P0, "Graveyard Bear", 2, 2)
        .with_mana_cost(ManaCost::generic(2))
        .id();
    for i in 0..3 {
        scenario.add_card_to_library_top(P0, &format!("Library Card {i}"));
    }
    UldarosBoard {
        runner: scenario.build(),
        uldaros,
        instant,
        second_instant,
        sorcery,
        bear,
    }
}

/// CR 115.1 + CR 205.2a + CR 707.12a + CR 202.3: one target per card type (a second
/// instant is refused), then the copies cast together must total mana value 6 or
/// less: instant (2) + bear (2) is fine, instant + sorcery (3) + bear (7) is not.
#[test]
fn uldaros_exiles_one_per_type_and_casts_copies_within_six() {
    let UldarosBoard {
        mut runner,
        uldaros,
        instant,
        second_instant,
        sorcery,
        bear,
    } = uldaros_board();
    let hand_before = runner.state().players[0].hand.len();
    let life_before = runner.life(P0);
    let card_id = runner.state().objects[&uldaros].card_id;
    runner
        .act(GameAction::CastSpell {
            object_id: uldaros,
            card_id,
            targets: vec![],
            payment_mode: CastPaymentMode::Auto,
        })
        .expect("Uldaros is castable");

    let mut picked = Vec::new();
    let mut refused_second_instant = false;
    let mut chose_copies = false;
    for _ in 0..60 {
        match runner.state().waiting_for.clone() {
            WaitingFor::TriggerTargetSelection {
                target_slots,
                selection,
                ..
            } => {
                let legal = &target_slots[selection.current_slot].legal_targets;
                if !refused_second_instant
                    && picked.contains(&instant)
                    && legal.contains(&TargetRef::Object(second_instant))
                {
                    assert!(
                        runner
                            .act(GameAction::ChooseTarget {
                                target: Some(TargetRef::Object(second_instant)),
                            })
                            .is_err(),
                        "two instants can't both be targeted (one per card type)"
                    );
                    refused_second_instant = true;
                    continue;
                }
                let next = [instant, sorcery, bear]
                    .into_iter()
                    .find(|id| !picked.contains(id) && legal.contains(&TargetRef::Object(*id)));
                if let Some(id) = next {
                    picked.push(id);
                }
                runner
                    .act(GameAction::ChooseTarget {
                        target: next.map(TargetRef::Object),
                    })
                    .expect("target accepted");
            }
            WaitingFor::ChooseFromZoneChoice { cards, .. } => {
                assert_eq!(
                    cards.len(),
                    3,
                    "the three exiled cards are offered: {cards:?}"
                );
                assert!(
                    runner
                        .act(GameAction::SelectCards {
                            cards: vec![instant, sorcery, bear],
                        })
                        .is_err(),
                    "total mana value 7 is over 6"
                );
                runner
                    .act(GameAction::SelectCards {
                        cards: vec![instant, bear],
                    })
                    .expect("total mana value 4 is fine");
                chose_copies = true;
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
    assert!(
        refused_second_instant,
        "the per-type constraint was exercised"
    );
    assert!(chose_copies, "the copies were offered");
    for id in [instant, sorcery, bear] {
        assert_eq!(
            runner.state().objects[&id].zone,
            Zone::Exile,
            "originals stay exiled"
        );
    }
    assert_eq!(
        runner.state().objects[&second_instant].zone,
        Zone::Graveyard
    );
    // Uldaros left the hand; the copied "Draw a card." drew one.
    assert_eq!(runner.state().players[0].hand.len(), hand_before - 1 + 1);
    assert_eq!(
        runner.life(P0),
        life_before,
        "the sorcery copy was not cast"
    );
    let bear_tokens = runner
        .state()
        .battlefield
        .iter()
        .filter(|id| {
            let obj = &runner.state().objects[*id];
            obj.name == "Graveyard Bear" && obj.is_token
        })
        .count();
    assert_eq!(
        bear_tokens, 1,
        "CR 707.10f: the cast creature copy becomes a token"
    );
}

/// The Borg Queen enters: the opponent exiles until a creature card, it enters
/// under P0's control with a +1/+1 counter as a Borg artifact creature, and the
/// rest go to the bottom of the opponent's library.
#[test]
fn borg_queen_assimilates_the_first_creature_and_bottoms_the_rest() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.with_mana_pool(
        P0,
        mana(&[
            ManaType::White,
            ManaType::Blue,
            ManaType::Black,
            ManaType::Colorless,
            ManaType::Colorless,
            ManaType::Colorless,
            ManaType::Colorless,
        ]),
    );
    let queen = {
        let mut b =
            scenario.add_creature_to_hand_from_oracle(P0, "The Borg Queen", 5, 5, BORG_QUEEN);
        b.as_artifact_creature().with_mana_cost(ManaCost::Cost {
            shards: vec![
                ManaCostShard::White,
                ManaCostShard::Blue,
                ManaCostShard::Black,
            ],
            generic: 4,
        });
        b.id()
    };
    // P1's library, top first: two non-creatures, then the creature, then one more.
    scenario.add_card_to_library_top(P1, "Bottom Filler");
    let found = scenario.add_card_to_library_top(P1, "Elf Scout");
    let top_b = scenario.add_card_to_library_top(P1, "Top Filler B");
    let top_a = scenario.add_card_to_library_top(P1, "Top Filler A");
    let mut runner = scenario.build();
    {
        let obj = runner.state_mut().objects.get_mut(&found).unwrap();
        obj.card_types
            .core_types
            .push(engine::types::card_type::CoreType::Creature);
        obj.card_types.subtypes.push("Elf".to_string());
        obj.base_card_types = obj.card_types.clone();
        obj.power = Some(1);
        obj.toughness = Some(1);
        obj.base_power = Some(1);
        obj.base_toughness = Some(1);
    }
    // The scenario has no card database, so seed the creature-type vocabulary the
    // "loses all other creature types" wipe consults (CR 205.3m).
    for creature_type in ["Elf", "Borg"] {
        let types = &mut runner.state_mut().all_creature_types;
        if !types.iter().any(|t| t == creature_type) {
            types.push(creature_type.to_string());
        }
    }
    let card_id = runner.state().objects[&queen].card_id;
    runner
        .act(GameAction::CastSpell {
            object_id: queen,
            card_id,
            targets: vec![],
            payment_mode: CastPaymentMode::Auto,
        })
        .expect("The Borg Queen is castable");
    for _ in 0..40 {
        match runner.state().waiting_for.clone() {
            WaitingFor::TriggerTargetSelection { .. } => {
                runner
                    .act(GameAction::ChooseTarget {
                        target: Some(TargetRef::Player(P1)),
                    })
                    .expect("target opponent");
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
    let state = runner.state();
    let obj = &state.objects[&found];
    assert_eq!(
        obj.zone,
        Zone::Battlefield,
        "the found creature card is assimilated"
    );
    assert_eq!(obj.controller, P0);
    assert!(obj.card_types.subtypes.iter().any(|s| s == "Borg"));
    assert!(
        !obj.card_types.subtypes.iter().any(|s| s == "Elf"),
        "loses all other creature types"
    );
    assert!(obj
        .card_types
        .core_types
        .contains(&engine::types::card_type::CoreType::Artifact));
    assert_eq!(
        obj.counters
            .get(&engine::types::counter::CounterType::Plus1Plus1)
            .copied(),
        Some(1)
    );
    for filler in [top_a, top_b] {
        assert_eq!(
            state.objects[&filler].zone,
            Zone::Library,
            "the rest go back"
        );
    }
    let lib = &state.players[1].library;
    let bottom_two: Vec<_> = lib.iter().rev().take(2).copied().collect();
    assert!(
        bottom_two.contains(&top_a) && bottom_two.contains(&top_b),
        "the rest are on the bottom: {lib:?}"
    );
}
