// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::collections::HashMap;

use serde_json::Value;

const EN: &str = include_str!("../locales/en.json");

pub const LOCALES: [(&str, &str); 32] = [
    ("en", EN),
    ("ar", include_str!("../locales/ar.json")),
    ("bg", include_str!("../locales/bg.json")),
    ("cs", include_str!("../locales/cs.json")),
    ("da", include_str!("../locales/da.json")),
    ("de", include_str!("../locales/de.json")),
    ("el", include_str!("../locales/el.json")),
    ("es", include_str!("../locales/es.json")),
    ("et", include_str!("../locales/et.json")),
    ("fi", include_str!("../locales/fi.json")),
    ("fr", include_str!("../locales/fr.json")),
    ("ga", include_str!("../locales/ga.json")),
    ("he", include_str!("../locales/he.json")),
    ("hi", include_str!("../locales/hi.json")),
    ("hr", include_str!("../locales/hr.json")),
    ("hu", include_str!("../locales/hu.json")),
    ("it", include_str!("../locales/it.json")),
    ("ja", include_str!("../locales/ja.json")),
    ("ko", include_str!("../locales/ko.json")),
    ("lt", include_str!("../locales/lt.json")),
    ("lv", include_str!("../locales/lv.json")),
    ("mt", include_str!("../locales/mt.json")),
    ("nl", include_str!("../locales/nl.json")),
    ("pl", include_str!("../locales/pl.json")),
    ("pt", include_str!("../locales/pt.json")),
    ("ro", include_str!("../locales/ro.json")),
    ("sk", include_str!("../locales/sk.json")),
    ("sl", include_str!("../locales/sl.json")),
    ("sv", include_str!("../locales/sv.json")),
    ("th", include_str!("../locales/th.json")),
    ("ua", include_str!("../locales/ua.json")),
    ("zh", include_str!("../locales/zh.json")),
];

pub struct Strings {
    lang: String,
    words: HashMap<String, String>,
    fallback: HashMap<String, String>,
    scripts: Option<Vec<[u8; 4]>>,
}

fn flatten(prefix: &str, value: &Value, out: &mut HashMap<String, String>) {
    match value {
        Value::Object(map) => {
            for (k, v) in map {
                let key = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                flatten(&key, v, out);
            }
        }
        Value::String(s) => {
            out.insert(prefix.to_string(), s.clone());
        }
        _ => {}
    }
}

fn table(json: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    if let Ok(v) = serde_json::from_str::<Value>(json) {
        flatten("", &v, &mut out);
    }
    out
}

fn code(lang: &str) -> &str {
    if matches!(lang, "zh" | "zh-CN" | "zh-Hans" | "zh-SG") {
        return "zh";
    }
    match lang.split(['-', '_']).next().unwrap_or("en") {
        "zh" => "en",
        "uk" => "ua",
        "iw" => "he",
        other => other,
    }
}

fn source(lang: &str) -> &'static str {
    let code = code(lang);
    LOCALES
        .iter()
        .find(|(c, _)| *c == code)
        .map_or(EN, |(_, json)| json)
}

pub fn translated(lang: &str) -> bool {
    !table(source(lang)).is_empty()
}

pub fn script(lang: &str) -> Option<[u8; 4]> {
    if matches!(lang, "zh" | "zh-CN" | "zh-Hans" | "zh-SG") {
        return Some(*b"Hani");
    }
    match lang.split(['-', '_']).next().unwrap_or("en") {
        "ja" => Some(*b"Hira"),
        "ko" => Some(*b"Hang"),
        "ar" => Some(*b"Arab"),
        "he" | "iw" => Some(*b"Hebr"),
        "th" => Some(*b"Thai"),
        "hi" => Some(*b"Deva"),
        _ => None,
    }
}

impl Strings {
    pub fn new(lang: &str) -> Self {
        Self {
            lang: lang.to_string(),
            words: table(source(lang)),
            fallback: table(EN),
            scripts: None,
        }
    }

    pub fn readable(lang: &str, scripts: &[[u8; 4]]) -> Self {
        let shown = script(lang).is_none_or(|s| scripts.contains(&s));
        Self {
            lang: lang.to_string(),
            words: table(if shown { source(lang) } else { EN }),
            fallback: table(EN),
            scripts: Some(scripts.to_vec()),
        }
    }

    pub fn lang(&self) -> &str {
        &self.lang
    }

    pub fn scripts(&self) -> Option<&[[u8; 4]]> {
        self.scripts.as_deref()
    }

    pub fn shows(&self, script: [u8; 4]) -> bool {
        self.scripts.as_ref().is_none_or(|s| s.contains(&script))
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.words
            .get(key)
            .or_else(|| self.fallback.get(key))
            .map(String::as_str)
    }

    pub fn t(&self, key: &str, text: &str) -> String {
        self.get(key).unwrap_or(text).to_string()
    }

    pub fn with(&self, key: &str, text: &str, vars: &[(&str, &str)]) -> String {
        let mut s = self.t(key, text);
        for (name, value) in vars {
            s = s.replace(&format!("{{{{{name}}}}}"), value);
        }
        s
    }
}

pub fn cached(lang: &str) -> &'static Strings {
    static CACHE: std::sync::Mutex<Vec<&'static Strings>> = std::sync::Mutex::new(Vec::new());
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(strings) = cache.iter().find(|s| s.lang == lang) {
        return strings;
    }
    let strings: &'static Strings = Box::leak(Box::new(Strings::new(lang)));
    cache.push(strings);
    strings
}

impl Default for Strings {
    fn default() -> Self {
        Self::new("en")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_keys_resolve_with_english_fallback() {
        let de = Strings::new("de");
        assert_eq!(de.t("settings.audio", "x"), "Audio");
        assert_eq!(de.t("settings.devices", "x"), "Geräte");
        assert_eq!(de.t("no.such.key", "Fallback"), "Fallback");
    }

    #[test]
    fn region_codes_pick_the_language() {
        assert_eq!(
            Strings::new("uk-UA").t("settings.audio", "x"),
            Strings::new("ua").t("settings.audio", "x")
        );
    }

    #[test]
    fn untranslated_languages_fall_back_to_english() {
        let en = Strings::new("en");
        for lang in [
            "zh-TW", "zh-Hant", "ja", "ko", "ar", "he", "th", "hi", "es", "pt", "pl", "da", "sv",
            "fi",
        ] {
            assert_eq!(
                Strings::new(lang).t("settings.audio", "x"),
                en.t("settings.audio", "x")
            );
        }
    }

    #[test]
    fn a_language_without_its_script_reads_english() {
        let no_font = Strings::readable("ja", &[*b"Hani"]);
        assert_eq!(no_font.lang(), "ja");
        assert!(!no_font.shows(*b"Hira"));
        assert_eq!(
            no_font.t("settings.audio", "x"),
            Strings::new("en").t("settings.audio", "x")
        );
        assert!(Strings::readable("ja", &[*b"Hira"]).shows(*b"Hira"));
        assert!(Strings::new("de").shows(*b"Arab"));
        assert_eq!(script("zh-TW"), None);
        assert_eq!(script("he"), Some(*b"Hebr"));
    }

    fn vars(text: &str) -> Vec<&str> {
        let mut out: Vec<&str> = text
            .split("{{")
            .skip(1)
            .filter_map(|rest| rest.split_once("}}").map(|(name, _)| name))
            .collect();
        out.sort_unstable();
        out
    }

    #[test]
    fn every_locale_is_empty_or_complete() {
        let en = table(EN);
        for (name, json) in LOCALES {
            assert!(
                serde_json::from_str::<Value>(json).is_ok(),
                "{name}.json is not valid JSON"
            );
            let words = table(json);
            if words.is_empty() {
                continue;
            }
            let missing: Vec<_> = en.keys().filter(|k| !words.contains_key(*k)).collect();
            let unknown: Vec<_> = words.keys().filter(|k| !en.contains_key(*k)).collect();
            let empty: Vec<_> = words
                .iter()
                .filter(|(_, v)| v.trim().is_empty())
                .map(|(k, _)| k)
                .collect();
            let placeholders: Vec<_> = en
                .iter()
                .filter(|(k, v)| words.get(*k).is_some_and(|w| vars(w) != vars(v)))
                .map(|(k, _)| k)
                .collect();
            assert!(
                missing.is_empty()
                    && unknown.is_empty()
                    && empty.is_empty()
                    && placeholders.is_empty(),
                "{name}.json: missing {missing:?}, unknown {unknown:?}, empty {empty:?}, placeholders differ in {placeholders:?}"
            );
        }
    }

    #[test]
    fn only_filled_locales_count_as_translated() {
        assert!(translated("en"));
        assert!(translated("de"));
        assert!(translated("uk-UA"));
        assert!(translated("zh"));
        assert!(translated("zh-CN"));
        assert!(!translated("it"));
    }

    #[test]
    fn placeholders_are_filled() {
        let en = Strings::new("en");
        assert_eq!(en.with("none", "Hi {{name}}", &[("name", "Ada")]), "Hi Ada");
    }
}
