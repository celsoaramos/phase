//! MagicFinder: die-roll result-table rows the main pipeline leaves as gaps.
//!
//! Runs over the finished parse (abilities and trigger bodies). Two shapes:
//!
//! * CR 706.3a + CR 111.1: a row "Create two of those tokens" refers to the
//!   token an EARLIER row of the same table creates (Swarming Goblins, Delina).
//!   The row is rewritten as a clone of that row's token effect with the row's
//!   count, through the shared `rewrite_those_tokens_from_antecedent` authority.
//! * CR 706.3a + CR 608.2c: "You may roll again" in a row (Delina, Wild Mage)
//!   is the optional "repeat this process" directive. The directive node is
//!   the existing `RuntimeHandled { RepeatProcess }`; the chain root loops on
//!   `RepeatDirectiveReached`, so the roll (and its table) runs again only when
//!   that row was reached and the controller accepted.
use crate::types::ability::{
    AbilityCondition, AbilityDefinition, Effect, EffectOutcomeSignal, RepeatContinuation,
    RuntimeHandler,
};

use super::lower::rewrite_those_tokens_from_antecedent;

/// Returns true when a "roll again" directive was installed in this chain, so
/// the caller marks the chain root as a loop.
fn fix_chain(def: &mut AbilityDefinition) -> bool {
    let mut repeat = false;
    if let Effect::RollDie { results, .. } = &mut *def.effect {
        for i in 0..results.len() {
            // CR 111.1: the antecedent is the nearest earlier row whose own
            // effect creates a token.
            let antecedent = (0..i).rev().find_map(|j| {
                let effect = &*results[j].effect.effect;
                matches!(effect, Effect::Token { .. } | Effect::CopyTokenOf { .. })
                    .then(|| effect.clone())
            });
            let row = &mut results[i].effect;
            if let Some(antecedent) = antecedent {
                rewrite_those_tokens_from_antecedent(&mut row.effect, &antecedent);
            }
            repeat |= install_roll_again(row);
        }
    }
    if let Some(sub) = def.sub_ability.as_deref_mut() {
        repeat |= fix_chain(sub);
    }
    repeat
}

/// CR 608.2c: replace a "roll again" gap in this row's chain with the repeat
/// directive, keeping its "you may" (`optional`).
fn install_roll_again(def: &mut AbilityDefinition) -> bool {
    let mut found = false;
    let mut node = Some(def);
    while let Some(cur) = node {
        let is_roll_again = cur
            .effect
            .unimplemented_description()
            .map(|d| {
                d.trim()
                    .trim_end_matches('.')
                    .eq_ignore_ascii_case("roll again")
            })
            .unwrap_or(false);
        if is_roll_again {
            *cur.effect = Effect::RuntimeHandled {
                handler: RuntimeHandler::RepeatProcess,
            };
            // Shown on the "you may" prompt (`OptionalEffectChoice.description`).
            cur.description = Some("Roll again".to_string());
            found = true;
        }
        node = cur.sub_ability.as_deref_mut();
    }
    found
}

fn fix_root(def: &mut AbilityDefinition) {
    if fix_chain(def) && def.repeat_until.is_none() {
        def.repeat_until = Some(RepeatContinuation::WhileCondition {
            condition: Box::new(AbilityCondition::EffectOutcome {
                signal: EffectOutcomeSignal::RepeatDirectiveReached,
            }),
            max_iterations: None,
        });
    }
}

pub(crate) fn fix_die_tables(
    abilities: &mut [AbilityDefinition],
    triggers: &mut [crate::types::ability::TriggerDefinition],
) {
    for ability in abilities {
        fix_root(ability);
    }
    for trigger in triggers {
        if let Some(execute) = trigger.execute.as_deref_mut() {
            fix_root(execute);
        }
    }
}
