//! The interface's words, from the same catalogs the web front end uses, so
//! a translation is still one JSON file per language for both shells.

use std::collections::HashMap;

use serde_json::Value;

const ENGLISH: &str = include_str!("../../ui-shell/frontend/src/i18n/locales/en.json");
const VIETNAMESE: &str = include_str!("../../ui-shell/frontend/src/i18n/locales/vi.json");
/// Picks the language; unset, English.
pub const LANGUAGE_ENV: &str = "FANSIPAN_LANG";

/// One language's messages, falling back to English for a key it lacks.
pub struct Words {
    chosen: HashMap<String, String>,
    english: HashMap<String, String>,
}

fn catalog(json: &str) -> HashMap<String, String> {
    match serde_json::from_str::<HashMap<String, Value>>(json) {
        Ok(entries) => entries
            .into_iter()
            .filter_map(|(key, value)| Some((key, value.as_str()?.to_string())))
            .collect(),
        Err(_) => HashMap::new(),
    }
}

impl Words {
    /// `language` is a catalog's tag ("en", "vi"); any other reads English.
    pub fn new(language: &str) -> Self {
        let chosen = match language {
            "vi" => catalog(VIETNAMESE),
            _ => HashMap::new(),
        };
        Self {
            chosen,
            english: catalog(ENGLISH),
        }
    }

    /// The message for `key` with each `{name}` in it filled from `vars`; the
    /// key itself when no catalog has it, so a missing word shows as such.
    pub fn t(&self, key: &str, vars: &[(&str, &str)]) -> String {
        let message = self
            .chosen
            .get(key)
            .or_else(|| self.english.get(key))
            .map_or(key, String::as_str);
        vars.iter()
            .fold(message.to_string(), |text, (name, value)| {
                text.replace(&format!("{{{name}}}"), value)
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_come_from_the_shared_catalogs_with_english_behind() {
        let english = Words::new("en");
        let vietnamese = Words::new("vi");

        assert_eq!(english.t("board.priceN", &[("n", "3")]), "Price 3");
        assert_ne!(vietnamese.t("board.ref", &[]), "board.ref");
        assert_eq!(vietnamese.t("no.such.key", &[]), "no.such.key");
        assert_eq!(
            Words::new("xx").t("board.ref", &[]),
            english.t("board.ref", &[])
        );
    }
}
