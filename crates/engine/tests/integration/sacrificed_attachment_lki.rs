//! A sacrificed Aura/Equipment's activated ability keeps its "enchanted/equipped
//! creature" (CR 608.2h + CR 113.7a).
//!
//! Uneasy Alliance: "{5}, Sacrifice this Aura: Exile enchanted creature. You create
//! a 1/1 black Ninja creature token." The Aura is sacrificed as a COST (CR 602.2b +
//! CR 601.2h), so by the time the ability resolves the zone-exit sever has already
//! cleared the Aura's live `attached_to`. Before the fix
//! `source_context_from_filter` read only that live field for a non-trigger source,
//! `FilterProp::EnchantedBy` took its "unattached Aura matches nothing" arm, and the
//! Ninja was created while the enchanted creature stayed on the battlefield.
//!
//! CR 608.2h: an effect that needs information from its source uses last known
//! information once the source has left the zone it was expected to be in. The
//! source's last battlefield departure row (`zone_changes_this_turn`) captured the
//! attachment before the sever.
//!
//! Oracle text below is verbatim from `client/public/card-data.json`.
//!
//! DISCRIMINATING: every `*_host_*` test asserts the enchanted/equipped creature is
//! affected; each fails at the pre-fix base (nothing happens to the host). The
//! `other` creature — carrying a DIFFERENT Aura — proves the look-back names exactly
//! the departed source's host, not "any enchanted creature". The CR 400.7 negative
//! is a fail-closed guard: once the host itself left, nothing else is touched.

use engine::game::game_object::AttachTarget;
use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::types::ability::ShieldKind;
use engine::types::actions::GameAction;
use engine::types::game_state::WaitingFor;
use engine::types::identifiers::ObjectId;
use engine::types::mana::{ManaType, ManaUnit};
use engine::types::phase::Phase;
use engine::types::statics::StaticMode;
use engine::types::zones::Zone;

const UNEASY_ALLIANCE: &str = "Enchant creature\nEnchanted creature can't attack or block.\n{5}, Sacrifice this Aura: Exile enchanted creature. You create a 1/1 black Ninja creature token. Activate only as a sorcery.";
const PHANTOM_WINGS: &str = "Enchant creature\nEnchanted creature has flying.\nSacrifice this Aura: Return enchanted creature to its owner's hand.";
const BRIAR_SHIELD: &str = "Enchant creature\nEnchanted creature gets +1/+1.\nSacrifice this Aura: Enchanted creature gets +3/+3 until end of turn.";
const CARAPACE: &str = "Enchant creature\nEnchanted creature gets +0/+2.\nSacrifice this Aura: Regenerate enchanted creature.";
const WINGS_OF_HUBRIS: &str = "Equipped creature has flying.\nSacrifice this Equipment: Equipped creature can't be blocked this turn. Sacrifice it at the beginning of the next end step.\nEquip {1} ({1}: Attach to target creature you control. Equip only as a sorcery.)";

#[derive(Clone, Copy)]
enum Kind {
    Aura,
    Equipment,
}

struct Board {
    runner: GameRunner,
    source: ObjectId,
    host: ObjectId,
    /// Enchanted by a DIFFERENT Aura — must never be affected.
    other: ObjectId,
}

fn attach(runner: &mut GameRunner, attachment: ObjectId, host: ObjectId) {
    runner
        .state_mut()
        .objects
        .get_mut(&attachment)
        .unwrap()
        .attached_to = Some(AttachTarget::Object(host));
    runner
        .state_mut()
        .objects
        .get_mut(&host)
        .unwrap()
        .attachments
        .push(attachment);
    runner.state_mut().layers_dirty.mark_full();
}

/// P0 controls `name` attached to P1's `Bear`; P1's `Other` wears an unrelated Aura.
fn board(kind: Kind, name: &str, text: &str, mana: usize) -> Board {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let host = scenario.add_creature(P1, "Bear", 2, 2).id();
    let other = scenario.add_creature(P1, "Other", 2, 2).id();
    let other_aura = scenario
        .add_enchantment_from_oracle(P1, "Other Aura", "")
        .with_subtypes(vec!["Aura"])
        .from_oracle_text_with_keywords(&["enchant"], "Enchant creature")
        .id();
    let source = match kind {
        Kind::Aura => scenario
            .add_enchantment_from_oracle(P0, name, "")
            .with_subtypes(vec!["Aura"])
            .from_oracle_text_with_keywords(&["enchant"], text)
            .id(),
        Kind::Equipment => scenario
            .add_artifact_from_oracle(P0, name, "")
            .with_subtypes(vec!["Equipment"])
            .from_oracle_text_with_keywords(&["equip"], text)
            .id(),
    };
    if mana > 0 {
        scenario.with_mana_pool(
            P0,
            (0..mana)
                .map(|_| ManaUnit::new(ManaType::Colorless, source, false, Vec::new()))
                .collect(),
        );
    }
    let mut runner = scenario.build();
    attach(&mut runner, source, host);
    attach(&mut runner, other_aura, other);
    runner.state_mut().waiting_for = WaitingFor::Priority { player: P0 };
    Board {
        runner,
        source,
        host,
        other,
    }
}

/// Activate the source's sacrifice-self ability and pay its costs, leaving the
/// ability on the stack.
fn activate_sacrifice(board: &mut Board) {
    let source = board.source;
    let index = board.runner.state().objects[&source]
        .abilities
        .iter()
        .position(|ability| {
            ability
                .description
                .as_deref()
                .is_some_and(|d| d.contains("Sacrifice ~"))
        })
        .expect("reach-guard: the sacrifice-self ability must parse");
    board
        .runner
        .act(GameAction::ActivateAbility {
            source_id: source,
            ability_index: index,
        })
        .expect("ActivateAbility must be accepted");
    for _ in 0..8 {
        match board.runner.state().waiting_for {
            WaitingFor::PayCost { .. } => {
                board
                    .runner
                    .act(GameAction::SelectCards {
                        cards: vec![source],
                    })
                    .expect("sacrifice-self cost must be accepted");
            }
            WaitingFor::ManaPayment { .. } => {
                board
                    .runner
                    .act(GameAction::PassPriority)
                    .expect("mana payment must finalize");
            }
            _ => break,
        }
    }
    assert_eq!(
        board.runner.state().objects[&source].zone,
        Zone::Graveyard,
        "reach-guard: the source must be sacrificed as the cost"
    );
    assert_eq!(
        board.runner.state().objects[&source].attached_to,
        None,
        "reach-guard: the zone-exit sever cleared the live attachment"
    );
    assert!(
        !board.runner.state().stack.is_empty(),
        "reach-guard: the ability must be on the stack"
    );
}

fn ninjas(runner: &GameRunner) -> usize {
    runner
        .state()
        .objects
        .values()
        .filter(|obj| {
            obj.is_token
                && obj.controller == P0
                && obj.zone == Zone::Battlefield
                && obj.name == "Ninja"
        })
        .count()
}

/// CR 608.2h + CR 303.4b: Uneasy Alliance exiles the creature it enchanted.
#[test]
fn uneasy_alliance_exiles_host_after_sacrificing_itself() {
    let mut b = board(Kind::Aura, "Uneasy Alliance", UNEASY_ALLIANCE, 5);
    activate_sacrifice(&mut b);
    b.runner.advance_until_stack_empty();

    let state = b.runner.state();
    assert_eq!(
        state.objects[&b.host].zone,
        Zone::Exile,
        "the enchanted creature must be exiled even though the Aura was sacrificed as the cost"
    );
    assert_eq!(
        state.objects[&b.other].zone,
        Zone::Battlefield,
        "a creature enchanted by a different Aura must not be exiled"
    );
    assert_eq!(ninjas(&b.runner), 1, "the activator creates one Ninja");
}

/// CR 608.2h + CR 303.4b: Phantom Wings returns the creature it enchanted.
#[test]
fn phantom_wings_returns_host_after_sacrificing_itself() {
    let mut b = board(Kind::Aura, "Phantom Wings", PHANTOM_WINGS, 0);
    activate_sacrifice(&mut b);
    b.runner.advance_until_stack_empty();

    let state = b.runner.state();
    assert_eq!(
        state.objects[&b.host].zone,
        Zone::Hand,
        "the enchanted creature must return to its owner's hand"
    );
    assert_eq!(state.objects[&b.other].zone, Zone::Battlefield);
}

/// CR 608.2h + CR 611.2c: Briar Shield's "+3/+3 until end of turn" lands on the
/// creature it enchanted (the static +1/+1 left with the Aura, so 2/2 → 5/5).
#[test]
fn briar_shield_pumps_host_after_sacrificing_itself() {
    let mut b = board(Kind::Aura, "Briar Shield", BRIAR_SHIELD, 0);
    activate_sacrifice(&mut b);
    b.runner.advance_until_stack_empty();

    let state = b.runner.state();
    assert_eq!(
        (
            state.objects[&b.host].power,
            state.objects[&b.host].toughness
        ),
        (Some(5), Some(5)),
        "the enchanted creature gets +3/+3"
    );
    assert_eq!(
        (
            state.objects[&b.other].power,
            state.objects[&b.other].toughness
        ),
        (Some(2), Some(2)),
        "a creature enchanted by a different Aura is not pumped"
    );
}

fn regeneration_shields(runner: &GameRunner, id: ObjectId) -> usize {
    runner.state().objects[&id]
        .replacement_definitions
        .as_slice()
        .iter()
        .filter(|def| def.shield_kind == ShieldKind::Regeneration && !def.is_consumed)
        .count()
}

/// CR 608.2h + CR 701.19a: Carapace regenerates the creature it enchanted.
#[test]
fn carapace_regenerates_host_after_sacrificing_itself() {
    let mut b = board(Kind::Aura, "Carapace", CARAPACE, 0);
    activate_sacrifice(&mut b);
    b.runner.advance_until_stack_empty();

    assert_eq!(
        regeneration_shields(&b.runner, b.host),
        1,
        "the enchanted creature gets a regeneration shield"
    );
    assert_eq!(regeneration_shields(&b.runner, b.other), 0);
}

fn cant_be_blocked(runner: &GameRunner, id: ObjectId) -> bool {
    runner.state().objects[&id]
        .static_definitions
        .as_slice()
        .iter()
        .any(|def| def.mode == StaticMode::CantBeBlocked)
}

/// CR 608.2h + CR 301.5a: Wings of Hubris (Equipment) makes the creature it
/// equipped unblockable this turn.
#[test]
fn wings_of_hubris_makes_equipped_host_unblockable_after_sacrificing_itself() {
    let mut b = board(Kind::Equipment, "Wings of Hubris", WINGS_OF_HUBRIS, 0);
    activate_sacrifice(&mut b);
    b.runner.advance_until_stack_empty();

    assert!(
        cant_be_blocked(&b.runner, b.host),
        "the equipped creature can't be blocked this turn"
    );
    assert!(!cant_be_blocked(&b.runner, b.other));
}

/// CR 400.7 + CR 608.2h: the host left the battlefield before the ability resolved.
/// "Enchanted creature" names that departed object only; it is a new object in the
/// graveyard, so nothing is exiled — not the dead host, and not another enchanted
/// creature. The rest of the ability (the Ninja) still resolves.
#[test]
fn uneasy_alliance_exiles_nothing_when_host_left_before_resolution() {
    let mut b = board(Kind::Aura, "Uneasy Alliance", UNEASY_ALLIANCE, 5);
    activate_sacrifice(&mut b);
    let mut events = Vec::new();
    engine::game::zones::move_to_zone(b.runner.state_mut(), b.host, Zone::Graveyard, &mut events);
    b.runner.advance_until_stack_empty();

    let state = b.runner.state();
    assert_eq!(
        state.objects[&b.host].zone,
        Zone::Graveyard,
        "the departed host is a new object (CR 400.7) and is not exiled from the graveyard"
    );
    assert_eq!(
        state.objects[&b.other].zone,
        Zone::Battlefield,
        "no other enchanted creature is exiled in the host's place"
    );
    assert_eq!(ninjas(&b.runner), 1, "the Ninja is still created");
}

/// CR 400.7: the departure look-back is bound to the ability's exact source
/// incarnation. The Aura left the battlefield (hand) and then moved again
/// (graveyard): an ability of the object it became on LEAVING the battlefield still
/// knows its host, but an ability of the later graveyard object — a different
/// object — must not inherit that old attachment.
#[test]
fn departed_attachment_lookback_is_bound_to_the_source_incarnation() {
    use engine::game::filter::{matches_target_filter, FilterContext};
    use engine::types::ability::{Effect, FilterProp, ResolvedAbility, TargetFilter, TypedFilter};

    let mut b = board(Kind::Aura, "Phantom Wings", PHANTOM_WINGS, 0);
    let mut events = Vec::new();
    engine::game::zones::move_to_zone(b.runner.state_mut(), b.source, Zone::Hand, &mut events);
    let departed_successor = b.runner.state().objects[&b.source].incarnation;
    engine::game::zones::move_to_zone(b.runner.state_mut(), b.source, Zone::Graveyard, &mut events);
    let later = b.runner.state().objects[&b.source].incarnation;
    assert_ne!(
        departed_successor, later,
        "reach-guard: the second move is a new object"
    );

    let enchanted_creature =
        TargetFilter::Typed(TypedFilter::creature().properties(vec![FilterProp::EnchantedBy]));
    let matches_host = |incarnation: u64| {
        let mut ability = ResolvedAbility::new(
            Effect::Unimplemented {
                name: "probe".to_string(),
                description: None,
            },
            vec![],
            b.source,
            P0,
        );
        ability.source_incarnation = Some(incarnation);
        let ctx = FilterContext::from_ability(&ability);
        matches_target_filter(b.runner.state(), b.host, &enchanted_creature, &ctx)
    };

    assert!(
        matches_host(departed_successor),
        "the object produced by the battlefield departure keeps its last known host"
    );
    assert!(
        !matches_host(later),
        "a later object with the same storage id must not inherit the old host"
    );
}

/// CR 608.2h + CR 113.7a: the other identity role — the Aura is still on the
/// battlefield when its (non-sacrifice) ability goes on the stack and is destroyed
/// in response. The ability's source incarnation is the departed battlefield object
/// itself, and "enchanted creature" is still the creature it last enchanted.
#[test]
fn aura_destroyed_in_response_still_pumps_its_last_host() {
    const PUMP_AURA: &str =
        "Enchant creature\n{1}: Enchanted creature gets +1/+1 until end of turn.";
    let mut b = board(Kind::Aura, "Test Pump Aura", PUMP_AURA, 1);
    let source = b.source;
    b.runner
        .act(GameAction::ActivateAbility {
            source_id: source,
            ability_index: 0,
        })
        .expect("ActivateAbility must be accepted");
    for _ in 0..8 {
        match b.runner.state().waiting_for {
            WaitingFor::ManaPayment { .. } => {
                b.runner
                    .act(GameAction::PassPriority)
                    .expect("mana payment must finalize");
            }
            _ => break,
        }
    }
    assert_eq!(
        b.runner.state().objects[&source].zone,
        Zone::Battlefield,
        "reach-guard: the Aura is still on the battlefield when its ability is on the stack"
    );
    assert!(
        !b.runner.state().stack.is_empty(),
        "reach-guard: the ability must be on the stack"
    );
    let mut events = Vec::new();
    engine::game::zones::move_to_zone(b.runner.state_mut(), source, Zone::Graveyard, &mut events);
    b.runner.advance_until_stack_empty();

    let state = b.runner.state();
    assert_eq!(
        (
            state.objects[&b.host].power,
            state.objects[&b.host].toughness
        ),
        (Some(3), Some(3)),
        "the creature the Aura last enchanted gets +1/+1"
    );
    assert_eq!(
        (
            state.objects[&b.other].power,
            state.objects[&b.other].toughness
        ),
        (Some(2), Some(2))
    );
}
