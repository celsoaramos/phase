//! blunder_audit — counts, per seat, the decisions an experienced player would
//! call a blunder on sight.
//!
//! The ladder measures WHO wins; this measures WHY a side loses games it should
//! not. It is a pure function over (state before the decision, the action taken):
//! nothing here touches the AI's own evaluation path, so it can be pointed at any
//! configuration, including a human transcript.
//!
//! Every detector is deliberately conservative — a false negative costs a data
//! point, a false positive poisons the table. When the rule is unsure (commander
//! damage, trample, hexproof, a choice prompt after a land play) it stays silent.

use std::collections::BTreeMap;

use engine::ai_support::legal_actions;
use engine::game::commander::commander_lethal_headroom;
use engine::game::players;
use engine::types::ability::{Effect, QuantityExpr, TargetFilter};
use engine::types::actions::GameAction;
use engine::types::card_type::CoreType;
use engine::types::game_state::{GameState, WaitingFor};
use engine::types::identifiers::ObjectId;
use engine::types::keywords::Keyword;
use engine::types::phase::Phase;
use engine::types::player::PlayerId;
use engine::types::zones::Zone;

use crate::cast_facts::cast_facts_for_object;
use crate::combat_ai::{
    battlefield_power, evaluate_block_outcome, is_lethal_attack_available, sum_power,
};
use crate::eval::evaluate_creature;

/// One class of blunder. `Ord` so the tally prints in a stable order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Blunder {
    /// A chump block (blocker dies, attacker survives) made with a creature when a
    /// cheaper legal blocker existed.
    ChumpWithBestCreature,
    /// A chump block while the defender's board hits at least as hard as the
    /// attacker's and the swing was not lethal.
    ChumpWhileRacing,
    /// A chump block with the swing far from lethal (life > 3 × attacker power).
    ChumpAtHighLife,
    /// Declared no (or a non-lethal) attack while an uncontested lethal attack
    /// was on the table.
    HeldLethal,
    /// Pointed single-target removal at a small creature while a creature worth
    /// at least twice as much, which the same spell could have killed, was there.
    RemovalOnSmallTarget,
    /// Passed priority in the own post-combat main phase with an empty stack
    /// and a castable creature or sorcery in hand.
    PassWithCastable,
    /// Played a land that enters tapped when an untapped land was also playable
    /// and would have let a spell in hand be cast this turn.
    TaplandNoSpell,
}

impl Blunder {
    pub fn all() -> &'static [Blunder] {
        &[
            Blunder::ChumpWithBestCreature,
            Blunder::ChumpWhileRacing,
            Blunder::ChumpAtHighLife,
            Blunder::HeldLethal,
            Blunder::RemovalOnSmallTarget,
            Blunder::PassWithCastable,
            Blunder::TaplandNoSpell,
        ]
    }

    pub fn label(&self) -> &'static str {
        match self {
            Blunder::ChumpWithBestCreature => "chump with best creature",
            Blunder::ChumpWhileRacing => "chump while racing",
            Blunder::ChumpAtHighLife => "chump at high life",
            Blunder::HeldLethal => "held lethal",
            Blunder::RemovalOnSmallTarget => "removal on small target",
            Blunder::PassWithCastable => "pass with castable",
            Blunder::TaplandNoSpell => "tapland, no spell",
        }
    }
}

/// Per-seat counts over a run of games.
#[derive(Debug, Default, Clone)]
pub struct AuditTally {
    pub per_seat: [BTreeMap<Blunder, u32>; 2],
    pub games: u32,
}

impl AuditTally {
    pub fn record(&mut self, seat: PlayerId, blunder: Blunder) {
        if let Some(map) = self.per_seat.get_mut(seat.0 as usize) {
            *map.entry(blunder).or_insert(0) += 1;
        }
    }

    pub fn count(&self, seat: PlayerId, blunder: Blunder) -> u32 {
        self.per_seat
            .get(seat.0 as usize)
            .and_then(|m| m.get(&blunder))
            .copied()
            .unwrap_or(0)
    }

    /// Fold another tally into this one (games played in parallel).
    pub fn merge(&mut self, other: &AuditTally) {
        for seat in 0..2 {
            for (b, n) in &other.per_seat[seat] {
                *self.per_seat[seat].entry(*b).or_insert(0) += n;
            }
        }
        self.games += other.games;
    }
}

/// Audit one decision: `pre` is the state the actor decided in, `action` what it
/// chose, `seat` who chose. Counts into `tally`; never mutates `pre`.
pub fn audit_step(pre: &GameState, action: &GameAction, seat: PlayerId, tally: &mut AuditTally) {
    if seat.0 as usize >= 2 {
        return;
    }
    match action {
        GameAction::DeclareBlockers { assignments } => {
            audit_blocks(pre, assignments, seat, tally);
        }
        GameAction::DeclareAttackers { attacks, .. } => {
            audit_attacks(pre, attacks.iter().map(|(id, _)| *id), seat, tally);
        }
        GameAction::CastSpell {
            object_id, targets, ..
        } => {
            audit_removal_target(pre, *object_id, targets, seat, tally);
        }
        GameAction::PassPriority => {
            audit_pass(pre, seat, tally);
        }
        GameAction::PlayLand { object_id, .. } => {
            audit_land_play(pre, *object_id, seat, tally);
        }
        _ => {}
    }
}

fn is_creature(state: &GameState, id: ObjectId) -> bool {
    state
        .objects
        .get(&id)
        .is_some_and(|o| o.card_types.core_types.contains(&CoreType::Creature))
}

fn power_of(state: &GameState, id: ObjectId) -> i32 {
    state
        .objects
        .get(&id)
        .and_then(|o| o.power)
        .unwrap_or(0)
        .max(0)
}

// ---------------------------------------------------------------------------
// Blocks
// ---------------------------------------------------------------------------

fn audit_blocks(
    pre: &GameState,
    assignments: &[(ObjectId, ObjectId)],
    seat: PlayerId,
    tally: &mut AuditTally,
) {
    let Some(combat) = pre.combat.as_ref() else {
        return;
    };
    // Attackers coming at THIS seat (a planeswalker attack still threatens the
    // board, but the life-total arithmetic below is about the player).
    let incoming: Vec<ObjectId> = combat
        .attackers
        .iter()
        .filter(|a| a.defending_player == seat)
        .map(|a| a.object_id)
        .collect();
    if incoming.is_empty() {
        return;
    }
    let life = pre.players[seat.0 as usize].life;
    let blocked: std::collections::HashSet<ObjectId> =
        assignments.iter().map(|&(_, a)| a).collect();
    let unblocked_power: i32 = incoming
        .iter()
        .filter(|a| !blocked.contains(a))
        .map(|&a| power_of(pre, a))
        .sum();
    let legal_blockers_for = |attacker: ObjectId| -> Vec<ObjectId> {
        if let WaitingFor::DeclareBlockers {
            valid_block_targets,
            ..
        } = &pre.waiting_for
        {
            valid_block_targets
                .iter()
                .filter(|(_, targets)| targets.contains(&attacker))
                .map(|(b, _)| *b)
                .collect()
        } else {
            pre.battlefield
                .iter()
                .copied()
                .filter(|&b| {
                    pre.objects.get(&b).is_some_and(|o| {
                        o.controller == seat
                            && !o.tapped
                            && o.card_types.core_types.contains(&CoreType::Creature)
                    }) && engine::game::combat::can_block_pair(pre, b, attacker)
                })
                .collect()
        }
    };
    let my_power = battlefield_power(pre, seat);

    for &(blocker_id, attacker_id) in assignments {
        // Only single blocks are judged — a gang member that dies is paying for
        // the kill the gang makes, not chumping.
        if assignments
            .iter()
            .filter(|&&(_, a)| a == attacker_id)
            .count()
            != 1
        {
            continue;
        }
        let (Some(blocker), Some(attacker)) =
            (pre.objects.get(&blocker_id), pre.objects.get(&attacker_id))
        else {
            continue;
        };
        if !incoming.contains(&attacker_id) {
            continue;
        }
        let (kills, survives) = evaluate_block_outcome(blocker, attacker);
        if kills || survives {
            continue;
        }
        // Commander damage changes what "lethal" means; stay silent there.
        if commander_lethal_headroom(pre, seat, attacker_id).is_some() {
            continue;
        }
        let attacker_power = power_of(pre, attacker_id);
        let blocker_value = evaluate_creature(pre, blocker_id);

        // (1) a cheaper body could have taken the hit.
        let cheaper_exists = legal_blockers_for(attacker_id).into_iter().any(|other| {
            other != blocker_id
                && !assignments.iter().any(|&(b, _)| b == other)
                && evaluate_creature(pre, other) + 0.5 < blocker_value
        });
        if cheaper_exists {
            tally.record(seat, Blunder::ChumpWithBestCreature);
        }

        // (2)/(3): the block was not needed to survive this swing.
        let incoming_if_unblocked = unblocked_power + attacker_power;
        if incoming_if_unblocked >= life {
            continue;
        }
        if life > attacker_power * 3 {
            tally.record(seat, Blunder::ChumpAtHighLife);
        }
        let attacker_side = battlefield_power(pre, attacker.controller);
        if my_power >= attacker_side && life > attacker_power * 2 {
            tally.record(seat, Blunder::ChumpWhileRacing);
        }
    }
}

// ---------------------------------------------------------------------------
// Attacks
// ---------------------------------------------------------------------------

fn audit_attacks(
    pre: &GameState,
    attackers: impl Iterator<Item = ObjectId>,
    seat: PlayerId,
    tally: &mut AuditTally,
) {
    if !is_lethal_attack_available(pre, seat) {
        return;
    }
    let declared: Vec<ObjectId> = attackers.collect();
    let min_opp_life = players::opponents(pre, seat)
        .iter()
        .map(|&opp| pre.players[opp.0 as usize].life)
        .min()
        .unwrap_or(i32::MAX);
    if sum_power(pre, &declared) < min_opp_life {
        tally.record(seat, Blunder::HeldLethal);
    }
}

// ---------------------------------------------------------------------------
// Removal target
// ---------------------------------------------------------------------------

/// The single-target destroy / damage effects a cast spell resolves with.
/// `None` when the spell is not (only) single-target creature removal.
fn removal_profile(effects: &[&Effect]) -> Option<(bool, Option<i32>)> {
    let mut destroy = false;
    let mut damage: Option<i32> = None;
    for e in effects {
        match e {
            Effect::Destroy { target, .. } if !matches!(target, TargetFilter::None) => {
                destroy = true;
            }
            Effect::DealDamage { amount, target, .. } if !matches!(target, TargetFilter::None) => {
                match amount {
                    QuantityExpr::Fixed { value } => {
                        damage = Some((*value).max(damage.unwrap_or(0)))
                    }
                    // Dynamic damage: the auditor cannot tell what the bigger
                    // creature would have survived — stay silent.
                    _ => return None,
                }
            }
            // Mass removal is not a targeting decision.
            Effect::DestroyAll { .. } | Effect::DamageAll { .. } => return None,
            _ => {}
        }
    }
    (destroy || damage.is_some()).then_some((destroy, damage))
}

fn audit_removal_target(
    pre: &GameState,
    spell_id: ObjectId,
    targets: &[ObjectId],
    seat: PlayerId,
    tally: &mut AuditTally,
) {
    let [target_id] = targets else {
        return;
    };
    let Some(spell) = pre.objects.get(&spell_id) else {
        return;
    };
    let facts = cast_facts_for_object(spell);
    let Some((destroys, damage)) = removal_profile(&facts.immediate_effects()) else {
        return;
    };
    let Some(target) = pre.objects.get(target_id) else {
        return;
    };
    if target.controller == seat
        || target.zone != Zone::Battlefield
        || !target.card_types.core_types.contains(&CoreType::Creature)
    {
        return;
    }
    let target_value = evaluate_creature(pre, *target_id);
    if target_value >= 3.0 {
        return;
    }
    let would_die = |id: ObjectId| -> bool {
        let Some(o) = pre.objects.get(&id) else {
            return false;
        };
        if o.has_keyword(&Keyword::Hexproof) || o.has_keyword(&Keyword::Shroud) {
            return false;
        }
        let by_destroy = destroys && !o.has_keyword(&Keyword::Indestructible);
        let by_damage = damage.is_some_and(|d| {
            !o.has_keyword(&Keyword::Indestructible)
                && o.toughness.unwrap_or(0) - i32::try_from(o.damage_marked).unwrap_or(0) <= d
        });
        by_destroy || by_damage
    };
    let bigger_exists = pre.battlefield.iter().any(|&id| {
        id != *target_id
            && pre
                .objects
                .get(&id)
                .is_some_and(|o| o.controller == target.controller)
            && is_creature(pre, id)
            && evaluate_creature(pre, id) >= 2.0 * target_value.max(1.0)
            && would_die(id)
    });
    if bigger_exists {
        tally.record(seat, Blunder::RemovalOnSmallTarget);
    }
}

// ---------------------------------------------------------------------------
// Pass with a castable spell
// ---------------------------------------------------------------------------

/// A `CastSpell` in `actions` whose card is a creature or sorcery in the actor's
/// hand. Instants and flash are held on purpose and never count.
fn castable_sorcery_speed(state: &GameState, actions: &[GameAction], seat: PlayerId) -> bool {
    actions.iter().any(|a| match a {
        GameAction::CastSpell { object_id, .. } => state.objects.get(object_id).is_some_and(|o| {
            o.controller == seat
                && o.zone == Zone::Hand
                && !o.has_keyword(&Keyword::Flash)
                && (o.card_types.core_types.contains(&CoreType::Creature)
                    || o.card_types.core_types.contains(&CoreType::Sorcery))
        }),
        _ => false,
    })
}

fn audit_pass(pre: &GameState, seat: PlayerId, tally: &mut AuditTally) {
    // Pre-combat passes are a legitimate "cast after combat"; only the post-combat
    // main phase has no later window this turn.
    if pre.phase != Phase::PostCombatMain || pre.active_player != seat || !pre.stack.is_empty() {
        return;
    }
    if !matches!(pre.waiting_for, WaitingFor::Priority { player } if player == seat) {
        return;
    }
    if castable_sorcery_speed(pre, &legal_actions(pre), seat) {
        tally.record(seat, Blunder::PassWithCastable);
    }
}

// ---------------------------------------------------------------------------
// Tapland when an untapped land would have enabled a spell
// ---------------------------------------------------------------------------

/// Plays `land` on a copy of `pre`. Returns (land entered tapped, a creature or
/// sorcery is castable afterwards); `None` when the play did not land back on a
/// plain priority (a choice prompt — Thriving lands — or an engine refusal).
fn simulate_land_play(pre: &GameState, land: ObjectId, seat: PlayerId) -> Option<(bool, bool)> {
    let card_id = pre.objects.get(&land)?.card_id;
    let mut after = pre.clone();
    engine::game::engine::apply_as_current_for_simulation(
        &mut after,
        GameAction::PlayLand {
            object_id: land,
            card_id,
        },
    )
    .ok()?;
    if !matches!(after.waiting_for, WaitingFor::Priority { player } if player == seat) {
        return None;
    }
    let obj = after.objects.get(&land)?;
    if obj.zone != Zone::Battlefield {
        return None;
    }
    let tapped = obj.tapped;
    let castable = castable_sorcery_speed(&after, &legal_actions(&after), seat);
    Some((tapped, castable))
}

fn audit_land_play(pre: &GameState, land: ObjectId, seat: PlayerId, tally: &mut AuditTally) {
    if pre.active_player != seat
        || !matches!(pre.phase, Phase::PreCombatMain | Phase::PostCombatMain)
        || !pre.stack.is_empty()
    {
        return;
    }
    let Some((entered_tapped, castable_after)) = simulate_land_play(pre, land, seat) else {
        return;
    };
    if !entered_tapped || castable_after {
        return;
    }
    let other_lands: Vec<ObjectId> = legal_actions(pre)
        .iter()
        .filter_map(|a| match a {
            GameAction::PlayLand { object_id, .. } if *object_id != land => Some(*object_id),
            _ => None,
        })
        .collect();
    let untapped_enables_spell = other_lands
        .into_iter()
        .any(|other| matches!(simulate_land_play(pre, other, seat), Some((false, true))));
    if untapped_enables_spell {
        tally.record(seat, Blunder::TaplandNoSpell);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::game::combat::{AttackTarget, AttackerInfo, CombatState};
    use engine::game::scenario::GameScenario;
    use engine::types::ability::{
        AbilityCost, AbilityDefinition, AbilityKind, EffectScope, ManaContribution, ManaProduction,
        ReplacementDefinition, TapStateChange,
    };
    use engine::types::mana::{ManaColor, ManaCost};
    use engine::types::replacements::ReplacementEvent;
    use std::collections::HashMap;

    const P0: PlayerId = PlayerId(0);
    const P1: PlayerId = PlayerId(1);

    /// P0 attacks P1 with `attackers`; the state is left at P1's blocker
    /// declaration (the fixture shape `ai_quality.rs` uses).
    fn at_blocks(runner: &mut engine::game::scenario::GameRunner, attackers: &[ObjectId]) {
        let state = runner.state_mut();
        state.phase = Phase::DeclareBlockers;
        state.active_player = P0;
        state.combat = Some(CombatState {
            attackers: attackers
                .iter()
                .map(|&a| AttackerInfo::attacking_player(a, P1))
                .collect(),
            ..Default::default()
        });
        let blockers: Vec<ObjectId> = state
            .battlefield
            .iter()
            .copied()
            .filter(|&id| {
                state.objects.get(&id).is_some_and(|o| {
                    o.controller == P1 && o.card_types.core_types.contains(&CoreType::Creature)
                })
            })
            .collect();
        let valid_block_targets: HashMap<ObjectId, Vec<ObjectId>> =
            blockers.iter().map(|&b| (b, attackers.to_vec())).collect();
        state.waiting_for = WaitingFor::DeclareBlockers {
            player: P1,
            valid_blocker_ids: blockers,
            valid_block_targets,
            block_requirements: HashMap::new(),
            blocker_constraints: Default::default(),
            must_be_blocked_targets: Default::default(),
            block_capacities: Default::default(),
        };
    }

    fn blocks(assignments: &[(ObjectId, ObjectId)]) -> GameAction {
        GameAction::DeclareBlockers {
            assignments: assignments.to_vec(),
        }
    }

    #[test]
    fn chump_with_the_fatty_is_flagged_and_with_the_token_is_not() {
        let mut scenario = GameScenario::new();
        scenario.with_life(P1, 20);
        let giant = scenario.add_creature(P0, "Giant", 7, 7).id();
        let token = scenario.add_creature(P1, "Token", 1, 1).id();
        let fatty = scenario.add_creature(P1, "Fatty", 5, 5).id();
        let mut runner = scenario.build();
        at_blocks(&mut runner, &[giant]);

        let mut tally = AuditTally::default();
        audit_step(runner.state(), &blocks(&[(fatty, giant)]), P1, &mut tally);
        assert_eq!(tally.count(P1, Blunder::ChumpWithBestCreature), 1);
        // 20 life vs a 7-power swing is inside 3 × power (21): not "high life".
        assert_eq!(tally.count(P1, Blunder::ChumpAtHighLife), 0);

        let mut tally = AuditTally::default();
        audit_step(runner.state(), &blocks(&[(token, giant)]), P1, &mut tally);
        assert_eq!(tally.count(P1, Blunder::ChumpWithBestCreature), 0);
        assert_eq!(tally.count(P0, Blunder::ChumpWithBestCreature), 0);
    }

    #[test]
    fn chump_at_high_life_and_while_racing() {
        // 20 life, lone 4/4 swings (20 > 12), defender's board 2/2 + 3/3 = 5 power
        // against the attacker's 4: racing AND high life.
        let mut scenario = GameScenario::new();
        scenario.with_life(P1, 20);
        let ogre = scenario.add_creature(P0, "Ogre", 4, 4).id();
        let bear = scenario.add_creature(P1, "Bear", 2, 2).id();
        let _knight = scenario.add_creature(P1, "Knight", 3, 3).id();
        let mut runner = scenario.build();
        at_blocks(&mut runner, &[ogre]);

        let mut tally = AuditTally::default();
        audit_step(runner.state(), &blocks(&[(bear, ogre)]), P1, &mut tally);
        assert_eq!(tally.count(P1, Blunder::ChumpAtHighLife), 1);
        assert_eq!(tally.count(P1, Blunder::ChumpWhileRacing), 1);
        // The bear IS the cheapest legal blocker.
        assert_eq!(tally.count(P1, Blunder::ChumpWithBestCreature), 0);

        // The same chump at 4 life is survival, not a blunder.
        runner.state_mut().players[1].life = 4;
        let mut tally = AuditTally::default();
        audit_step(runner.state(), &blocks(&[(bear, ogre)]), P1, &mut tally);
        assert_eq!(tally.count(P1, Blunder::ChumpAtHighLife), 0);
        assert_eq!(tally.count(P1, Blunder::ChumpWhileRacing), 0);
    }

    #[test]
    fn a_block_that_kills_or_survives_is_never_a_chump() {
        let mut scenario = GameScenario::new();
        scenario.with_life(P1, 20);
        let bear = scenario.add_creature(P0, "Bear", 2, 2).id();
        let wall = scenario.add_creature(P1, "Wall", 0, 4).id();
        let token = scenario.add_creature(P1, "Token", 1, 1).id();
        let mut runner = scenario.build();
        at_blocks(&mut runner, &[bear]);

        let mut tally = AuditTally::default();
        audit_step(runner.state(), &blocks(&[(wall, bear)]), P1, &mut tally);
        assert!(tally.per_seat[1].is_empty(), "{tally:?}");
        let _ = token;
    }

    #[test]
    fn held_lethal_is_flagged_only_with_an_open_lethal_swing() {
        let mut scenario = GameScenario::new();
        scenario.at_phase(Phase::DeclareAttackers);
        scenario.with_life(P1, 3);
        let bear = scenario.add_creature(P0, "Bear", 3, 3).id();
        let runner = scenario.build();
        assert!(is_lethal_attack_available(runner.state(), P0));

        let mut tally = AuditTally::default();
        let hold = GameAction::DeclareAttackers {
            attacks: vec![],
            bands: vec![],
        };
        audit_step(runner.state(), &hold, P0, &mut tally);
        assert_eq!(tally.count(P0, Blunder::HeldLethal), 1);

        let mut tally = AuditTally::default();
        let swing = GameAction::DeclareAttackers {
            attacks: vec![(bear, AttackTarget::Player(P1))],
            bands: vec![],
        };
        audit_step(runner.state(), &swing, P0, &mut tally);
        assert_eq!(tally.count(P0, Blunder::HeldLethal), 0);
    }

    #[test]
    fn holding_back_is_fine_when_a_blocker_is_untapped() {
        let mut scenario = GameScenario::new();
        scenario.at_phase(Phase::DeclareAttackers);
        scenario.with_life(P1, 3);
        scenario.add_creature(P0, "Bear", 3, 3);
        scenario.add_creature(P1, "Wall", 0, 4);
        let runner = scenario.build();
        let mut tally = AuditTally::default();
        let hold = GameAction::DeclareAttackers {
            attacks: vec![],
            bands: vec![],
        };
        audit_step(runner.state(), &hold, P0, &mut tally);
        assert_eq!(tally.count(P0, Blunder::HeldLethal), 0);
    }

    #[test]
    fn bolt_on_the_token_next_to_a_bear_the_bolt_kills_is_flagged() {
        let mut scenario = GameScenario::new();
        scenario.at_phase(Phase::PreCombatMain);
        let bolt = scenario.add_bolt_to_hand(P0);
        let token = scenario.add_creature(P1, "Token", 1, 1).id();
        let bear = scenario.add_creature(P1, "Bear", 3, 3).id();
        let runner = scenario.build();
        let state = runner.state();
        assert!(evaluate_creature(state, token) < 3.0);
        assert!(evaluate_creature(state, bear) >= 2.0 * evaluate_creature(state, token));
        let card_id = state.objects.get(&bolt).unwrap().card_id;

        let cast = |t: ObjectId| GameAction::CastSpell {
            object_id: bolt,
            card_id,
            targets: vec![t],
            payment_mode: Default::default(),
        };
        let mut tally = AuditTally::default();
        audit_step(state, &cast(token), P0, &mut tally);
        assert_eq!(tally.count(P0, Blunder::RemovalOnSmallTarget), 1);

        let mut tally = AuditTally::default();
        audit_step(state, &cast(bear), P0, &mut tally);
        assert_eq!(tally.count(P0, Blunder::RemovalOnSmallTarget), 0);
    }

    #[test]
    fn bolt_on_the_token_is_fine_when_the_bigger_creature_survives_three() {
        let mut scenario = GameScenario::new();
        scenario.at_phase(Phase::PreCombatMain);
        let bolt = scenario.add_bolt_to_hand(P0);
        let token = scenario.add_creature(P1, "Token", 1, 1).id();
        scenario.add_creature(P1, "Wurm", 6, 6);
        let runner = scenario.build();
        let state = runner.state();
        let card_id = state.objects.get(&bolt).unwrap().card_id;
        let mut tally = AuditTally::default();
        audit_step(
            state,
            &GameAction::CastSpell {
                object_id: bolt,
                card_id,
                targets: vec![token],
                payment_mode: Default::default(),
            },
            P0,
            &mut tally,
        );
        assert_eq!(tally.count(P0, Blunder::RemovalOnSmallTarget), 0);
    }

    fn two_lands_and_a_two_drop(phase: Phase) -> (GameScenario, ObjectId) {
        let mut scenario = GameScenario::new();
        scenario.at_phase(phase);
        scenario.add_basic_land(P0, ManaColor::Green);
        scenario.add_basic_land(P0, ManaColor::Green);
        let creature = scenario
            .add_creature_to_hand(P0, "Grizzly", 2, 2)
            .with_mana_cost(ManaCost::generic(2))
            .id();
        (scenario, creature)
    }

    #[test]
    fn passing_post_combat_with_a_castable_creature_is_flagged() {
        let (scenario, creature) = two_lands_and_a_two_drop(Phase::PostCombatMain);
        let runner = scenario.build();
        let actions = legal_actions(runner.state());
        assert!(
            actions.iter().any(
                |a| matches!(a, GameAction::CastSpell { object_id, .. } if *object_id == creature)
            ),
            "fixture: the two-drop must be castable, got {actions:?}"
        );
        let mut tally = AuditTally::default();
        audit_step(runner.state(), &GameAction::PassPriority, P0, &mut tally);
        assert_eq!(tally.count(P0, Blunder::PassWithCastable), 1);
    }

    #[test]
    fn passing_pre_combat_or_without_mana_is_not_flagged() {
        let (scenario, _) = two_lands_and_a_two_drop(Phase::PreCombatMain);
        let runner = scenario.build();
        let mut tally = AuditTally::default();
        audit_step(runner.state(), &GameAction::PassPriority, P0, &mut tally);
        assert_eq!(tally.count(P0, Blunder::PassWithCastable), 0);

        let mut scenario = GameScenario::new();
        scenario.at_phase(Phase::PostCombatMain);
        scenario.add_basic_land(P0, ManaColor::Green);
        scenario
            .add_creature_to_hand(P0, "Grizzly", 2, 2)
            .with_mana_cost(ManaCost::generic(2));
        let runner = scenario.build();
        let mut tally = AuditTally::default();
        audit_step(runner.state(), &GameAction::PassPriority, P0, &mut tally);
        assert_eq!(tally.count(P0, Blunder::PassWithCastable), 0);
    }

    fn green_mana_ability() -> AbilityDefinition {
        AbilityDefinition::new(
            AbilityKind::Activated,
            Effect::Mana {
                produced: ManaProduction::Fixed {
                    colors: vec![ManaColor::Green],
                    contribution: ManaContribution::Base,
                },
                restrictions: vec![],
                grants: vec![],
                expiry: None,
                target: None,
            },
        )
        .cost(AbilityCost::Tap)
    }

    fn enters_tapped() -> ReplacementDefinition {
        ReplacementDefinition::new(ReplacementEvent::Moved)
            .execute(AbilityDefinition::new(
                AbilityKind::Spell,
                Effect::SetTapState {
                    target: TargetFilter::SelfRef,
                    scope: EffectScope::Single,
                    state: TapStateChange::Tap,
                },
            ))
            .valid_card(TargetFilter::SelfRef)
            .description("enters the battlefield tapped.".to_string())
    }

    /// One Forest on the battlefield, a Forest and a Guildgate in hand, a
    /// two-drop in hand: the Forest enables the two-drop, the gate does not.
    fn tapland_fixture() -> (GameScenario, ObjectId, ObjectId) {
        let mut scenario = GameScenario::new();
        scenario.at_phase(Phase::PreCombatMain);
        scenario.add_basic_land(P0, ManaColor::Green);
        let forest = scenario
            .add_land_to_hand(P0, "Forest")
            .with_ability_definition(green_mana_ability())
            .id();
        let gate = scenario
            .add_land_to_hand(P0, "Selesnya Guildgate")
            .with_ability_definition(green_mana_ability())
            .with_replacement_definition(enters_tapped())
            .id();
        scenario
            .add_creature_to_hand(P0, "Grizzly", 2, 2)
            .with_mana_cost(ManaCost::generic(2));
        (scenario, forest, gate)
    }

    fn play(state: &GameState, land: ObjectId) -> GameAction {
        GameAction::PlayLand {
            object_id: land,
            card_id: state.objects.get(&land).unwrap().card_id,
        }
    }

    #[test]
    fn tapland_over_an_enabling_untapped_land_is_flagged() {
        let (scenario, forest, gate) = tapland_fixture();
        let runner = scenario.build();
        let state = runner.state();
        assert_eq!(
            simulate_land_play(state, gate, P0),
            Some((true, false)),
            "fixture: the gate enters tapped and leaves the two-drop uncastable"
        );
        assert_eq!(simulate_land_play(state, forest, P0), Some((false, true)));

        let mut tally = AuditTally::default();
        audit_step(state, &play(state, gate), P0, &mut tally);
        assert_eq!(tally.count(P0, Blunder::TaplandNoSpell), 1);

        let mut tally = AuditTally::default();
        audit_step(state, &play(state, forest), P0, &mut tally);
        assert_eq!(tally.count(P0, Blunder::TaplandNoSpell), 0);
    }

    #[test]
    fn tapland_is_fine_when_no_spell_hinges_on_it() {
        // Same hand, no two-drop: nothing to cast either way.
        let mut scenario = GameScenario::new();
        scenario.at_phase(Phase::PreCombatMain);
        scenario.add_basic_land(P0, ManaColor::Green);
        scenario
            .add_land_to_hand(P0, "Forest")
            .with_ability_definition(green_mana_ability());
        let gate = scenario
            .add_land_to_hand(P0, "Selesnya Guildgate")
            .with_ability_definition(green_mana_ability())
            .with_replacement_definition(enters_tapped())
            .id();
        let runner = scenario.build();
        let state = runner.state();
        let mut tally = AuditTally::default();
        audit_step(state, &play(state, gate), P0, &mut tally);
        assert_eq!(tally.count(P0, Blunder::TaplandNoSpell), 0);
    }

    #[test]
    fn tally_merge_adds_per_seat() {
        let mut a = AuditTally::default();
        a.record(P0, Blunder::HeldLethal);
        a.games = 1;
        let mut b = AuditTally::default();
        b.record(P0, Blunder::HeldLethal);
        b.record(P1, Blunder::TaplandNoSpell);
        b.games = 2;
        a.merge(&b);
        assert_eq!(a.count(P0, Blunder::HeldLethal), 2);
        assert_eq!(a.count(P1, Blunder::TaplandNoSpell), 1);
        assert_eq!(a.games, 3);
        assert_eq!(Blunder::all().len(), 7);
        assert!(Blunder::all().iter().all(|b| !b.label().is_empty()));
    }
}
