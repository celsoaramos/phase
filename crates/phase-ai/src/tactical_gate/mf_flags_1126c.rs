//! Station past its last threshold (MagicFinder "jogada ruim do robô" 6eec8f76,
//! 26/11/2026).
//!
//! In its precombat main phase the robot activated Station on Class-4 Tactical
//! Cube (88 charge counters, every STATION threshold at 7+) and could do the same
//! on U.S.S. Prometheus, Prototype (13 counters, thresholds 4+ and 8+). CR 702.184a:
//! Station taps another untapped creature to put charge counters on the
//! Spacecraft; CR 721.2b: its STATION N+ abilities only care about reaching N.
//! Once every threshold is met, more charge counters change nothing, and the
//! tapped creature is a lost attacker or blocker.
//!
//! The gate rejects only what it can PROVE: the Spacecraft has at least one
//! threshold, all of them are met, no other text on it reads charge counters, and
//! nothing the robot controls triggers on counters being put. Otherwise it fails
//! open.

use engine::types::ability::{ActivationRestriction, StaticCondition, TriggerCondition};
use engine::types::actions::GameAction;
use engine::types::counter::{CounterMatch, CounterType};
use engine::types::game_state::{GameState, WaitingFor};
use engine::types::identifiers::ObjectId;
use engine::types::player::PlayerId;

use super::GateDecision;
use crate::policies::context::PolicyContext;

/// `ActivateStation` announced from priority. `Some(Reject)` removes the candidate.
pub(super) fn assess_station(ctx: &PolicyContext<'_>) -> Option<GateDecision> {
    let GameAction::ActivateStation { spacecraft_id, .. } = &ctx.candidate.action else {
        return None;
    };
    // Only the announcement; picking the creature (`WaitingFor::StationTarget`)
    // comes after the decision to station was already taken.
    if !matches!(ctx.decision.waiting_for, WaitingFor::Priority { .. }) {
        return None;
    }
    if !station_is_exhausted(ctx.state, ctx.ai_player, *spacecraft_id) {
        return None;
    }
    if std::env::var_os("MF_GATE_TRACE").is_some() {
        eprintln!("mf_flags_1126c reject ActivateStation {spacecraft_id:?}: every threshold met");
    }
    Some(GateDecision::Reject)
}

/// True when more charge counters on `spacecraft_id` provably change nothing.
pub fn station_is_exhausted(state: &GameState, player: PlayerId, spacecraft_id: ObjectId) -> bool {
    let Some((current, thresholds)) = station_thresholds(state, spacecraft_id) else {
        return false;
    };
    let Some(highest) = thresholds.iter().copied().max() else {
        return false;
    };
    if current < highest {
        return false;
    }
    let Some(spacecraft) = state.objects.get(&spacecraft_id) else {
        return false;
    };
    !reads_charge_counters(spacecraft) && !counter_trigger_in_play(state, player)
}

/// Any ability text on the Spacecraft that mentions charge counters outside the
/// threshold conditions ("for each charge counter", "remove a charge counter").
fn reads_charge_counters(spacecraft: &engine::game::game_object::GameObject) -> bool {
    let mentions = |value: serde_json::Value| value.to_string().to_lowercase().contains("charge");
    let statics = spacecraft.static_definitions.as_slice().iter().any(|def| {
        let mut def = def.clone();
        def.condition = None;
        serde_json::to_value(&def).map_or(true, mentions)
    });
    let triggers = spacecraft
        .trigger_definitions
        .as_slice()
        .iter()
        .any(|entry| {
            let mut def = entry.definition.clone();
            def.condition = None;
            serde_json::to_value(&def).map_or(true, mentions)
        });
    let abilities = spacecraft.abilities.iter().any(|def| {
        let mut def = def.clone();
        def.activation_restrictions.clear();
        serde_json::to_value(&def).map_or(true, mentions)
    });
    statics || triggers || abilities
}

/// A permanent of `player` that triggers on counters being put ("whenever one or
/// more counters are put on …") would still get value from Station.
fn counter_trigger_in_play(state: &GameState, player: PlayerId) -> bool {
    state.battlefield.iter().any(|id| {
        state.objects.get(id).is_some_and(|obj| {
            obj.controller == player
                && obj.trigger_definitions.as_slice().iter().any(|entry| {
                    serde_json::to_value(&entry.definition.mode)
                        .map_or(true, |mode| mode.to_string().contains("CounterAdded"))
                })
        })
    })
}

/// CR 721.2a-b: the charge counters on a Spacecraft and every STATION N+
/// threshold printed on it (static, trigger and activation thresholds) — the
/// same reading `policies::payment_selection` uses for the next threshold.
fn station_thresholds(state: &GameState, spacecraft_id: ObjectId) -> Option<(u32, Vec<u32>)> {
    let charge = CounterType::Generic("charge".to_string());
    let is_charge = |counters: &CounterMatch| counters == &CounterMatch::OfType(charge.clone());
    let spacecraft = state.objects.get(&spacecraft_id)?;
    let current = spacecraft.counters.get(&charge).copied().unwrap_or(0);
    let mut thresholds = Vec::new();
    for def in spacecraft.static_definitions.as_slice() {
        if let Some(StaticCondition::HasCounters {
            counters,
            minimum,
            maximum: None,
        }) = &def.condition
        {
            if is_charge(counters) {
                thresholds.push(*minimum);
            }
        }
    }
    for entry in spacecraft.trigger_definitions.as_slice() {
        if let Some(TriggerCondition::HasCounters {
            counters,
            minimum,
            maximum: None,
        }) = &entry.definition.condition
        {
            if is_charge(counters) {
                thresholds.push(*minimum);
            }
        }
    }
    for def in spacecraft.abilities.iter() {
        for restriction in &def.activation_restrictions {
            if let ActivationRestriction::CounterThreshold {
                counters,
                minimum,
                maximum: None,
            } = restriction
            {
                if is_charge(counters) {
                    thresholds.push(*minimum);
                }
            }
        }
    }
    Some((current, thresholds))
}
