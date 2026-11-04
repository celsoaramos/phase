//! House-rule emblems ("regra da semana"): Oracle texts carried on
//! `FormatConfig::house_emblems` that become game-start emblems in the command
//! zone.
//!
//! CR 114.1 + CR 114.4: an emblem is an object in the command zone whose
//! abilities function from there; a static ability such as "All creatures
//! have trample." therefore applies to every creature on the battlefield,
//! including tokens and creatures that enter later (CR 611.3a — a static
//! ability's continuous effect applies to whatever matches at each moment).
//!
//! Fail-closed contract: a text the engine's Oracle parser does not fully
//! understand is an ERROR, never an empty emblem. `compile_house_emblems` is
//! the single authority; `FormatConfig::deserialize` calls it so every
//! external ingress (WASM, server, lobby, save/restore) rejects a bad config
//! before a game can start, and `grant_house_emblems` re-runs it at game
//! start.

use crate::game::coverage::static_has_unimplemented_parts;
use crate::parser::oracle::parse_oracle_text;
use crate::types::ability::StaticDefinition;
use crate::types::game_state::GameState;
use crate::types::player::PlayerId;

/// Display name for house-rule emblems (no card is the source of them).
pub const HOUSE_EMBLEM_SOURCE_NAME: &str = "House Rule";

/// Compile each house-emblem text into the static abilities its emblem will
/// carry. Returns one `Vec<StaticDefinition>` per text, in input order.
///
/// A text is accepted only when the Oracle parser turns it into one or more
/// static abilities and nothing else: no activated/triggered/replacement
/// abilities, keywords, casting metadata, parse warnings, or unimplemented
/// fragments anywhere inside the statics.
pub fn compile_house_emblems(texts: &[String]) -> Result<Vec<Vec<StaticDefinition>>, String> {
    texts.iter().map(|text| compile_one(text)).collect()
}

fn compile_one(text: &str) -> Result<Vec<StaticDefinition>, String> {
    let reject = |why: &str| {
        Err(format!(
            "FormatConfig.house_emblems: the engine cannot apply house emblem {text:?} — {why}"
        ))
    };
    if text.trim().is_empty() {
        return reject("the text is empty");
    }
    let parsed = parse_oracle_text(text, HOUSE_EMBLEM_SOURCE_NAME, &[], &[], &[]);
    if !parsed.parse_warnings.is_empty() {
        return reject("the Oracle parser reported warnings for it");
    }
    let has_non_static_output = !parsed.abilities.is_empty()
        || !parsed.triggers.is_empty()
        || !parsed.replacements.is_empty()
        || !parsed.extracted_keywords.is_empty()
        || parsed.modal.is_some()
        || parsed.additional_cost.is_some()
        || !parsed.casting_restrictions.is_empty()
        || !parsed.casting_options.is_empty()
        || parsed.solve_condition.is_some()
        || parsed.strive_cost.is_some();
    if has_non_static_output {
        return reject("only static abilities are supported on house emblems");
    }
    if parsed.statics.is_empty() {
        return reject("the Oracle parser did not recognize it as a static ability");
    }
    if parsed.statics.iter().any(static_has_unimplemented_parts) {
        return reject("the Oracle parser left part of it unimplemented");
    }
    Ok(parsed.statics)
}

/// CR 114.1 + CR 114.4: create one emblem per `state.format_config.house_emblems`
/// text in the command zone before the game begins.
///
/// An emblem must have an owner in this engine; house emblems belong to no
/// player in spirit, so they are owned by `PlayerId(0)` (the same convention
/// as the shared planar deck). Their texts only use "all creatures"-style
/// subjects, which do not depend on who controls the emblem.
///
/// Panics only if an in-process caller built a `FormatConfig` with an
/// unparseable text directly (every serialized ingress is rejected earlier by
/// `FormatConfig::deserialize`): starting a "house rule" game that silently
/// plays as a normal game is never acceptable.
pub fn grant_house_emblems(state: &mut GameState) {
    if state.format_config.house_emblems.is_empty() {
        return;
    }
    let compiled = compile_house_emblems(&state.format_config.house_emblems)
        .unwrap_or_else(|error| panic!("{error}"));
    for statics in compiled {
        let emblem_id = crate::game::effects::create_emblem::grant_emblem(
            state,
            PlayerId(0),
            statics,
            Vec::new(),
            Vec::new(),
        );
        if let Some(obj) = state.objects.get_mut(&emblem_id) {
            obj.emblem_source = Some(crate::game::game_object::EmblemSource {
                name: HOUSE_EMBLEM_SOURCE_NAME.to_string(),
                printed_ref: None,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEASON_TEXTS: [&str; 7] = [
        "All creatures have trample.",
        "All creatures have deathtouch.",
        "All creatures have lifelink.",
        "All creatures have haste.",
        "All creatures have flying.",
        "All creatures have vigilance.",
        "All creatures have double strike.",
    ];

    #[test]
    fn season_texts_compile_to_statics() {
        for text in SEASON_TEXTS {
            let compiled = compile_house_emblems(&[text.to_string()])
                .unwrap_or_else(|e| panic!("{text}: {e}"));
            assert_eq!(compiled.len(), 1);
            assert!(!compiled[0].is_empty(), "{text}");
        }
    }

    #[test]
    fn unparseable_text_is_rejected() {
        for text in ["", "   ", "Flibbertigibbet the wombat.", "Draw a card."] {
            assert!(
                compile_house_emblems(&[text.to_string()]).is_err(),
                "{text:?} must be rejected"
            );
        }
    }
}
