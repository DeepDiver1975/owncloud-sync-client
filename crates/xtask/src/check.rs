use crate::locale::{self, LocaleMap};
use crate::source_scan;
use std::collections::BTreeSet;
use std::path::Path;

pub const LOCALES_DIR: &str = "crates/gui/locales";
pub const SOURCE_DIR: &str = "crates/gui/src";
pub const LOCALE_NAMES: &[&str] = &["en", "de", "fr", "zh"];

pub fn collect_violations(
    source_keys: &BTreeSet<String>,
    locales: &LocaleMap,
    hardcoded: &[(String, String)],
) -> Vec<String> {
    let mut violations = Vec::new();

    // Keys used in source but missing from a locale file
    for locale_name in LOCALE_NAMES {
        if !locales.contains_key(*locale_name) {
            continue;
        }
        let locale_keys = locale::keys_for_locale(locales, locale_name);
        for key in source_keys {
            if !locale_keys.contains(key.as_str()) {
                violations.push(format!(
                    "missing: key '{}' not found in {}.yml",
                    key, locale_name
                ));
            }
        }
    }

    // Keys in locale files not used in source
    for (locale_name, keys) in locales {
        for key in keys.keys() {
            if !source_keys.contains(key) {
                violations.push(format!(
                    "unused: key '{}' in {}.yml is not referenced in source",
                    key, locale_name
                ));
            }
        }
    }

    // Hardcoded visible strings
    for (file, s) in hardcoded {
        violations.push(format!("hardcoded: '{}' in {} should use t!()", s, file));
    }

    violations
}

pub fn run() -> anyhow::Result<()> {
    let src_dir = Path::new(SOURCE_DIR);
    let locales_dir = Path::new(LOCALES_DIR);

    let source_keys = source_scan::scan_source_keys(src_dir)?;
    let hardcoded = source_scan::scan_hardcoded_strings(src_dir)?;

    let mut all_locales = LocaleMap::new();
    for name in LOCALE_NAMES {
        let path = locales_dir.join(format!("{}.yml", name));
        let (map, _) = locale::load_locale_file(&path)?;
        if let Some(keys) = map.into_values().next() {
            all_locales.insert(name.to_string(), keys);
        }
    }

    let violations = collect_violations(&source_keys, &all_locales, &hardcoded);

    if violations.is_empty() {
        println!("check-keys: OK");
        return Ok(());
    }

    for v in &violations {
        eprintln!("{}", v);
    }
    anyhow::bail!("{} violation(s) found", violations.len());
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};

    fn make_locale(keys: &[&str]) -> BTreeMap<String, String> {
        keys.iter()
            .map(|k| (k.to_string(), "val".to_string()))
            .collect()
    }

    #[test]
    fn detects_missing_key_in_locale() {
        let source_keys: BTreeSet<String> = ["cancel_btn", "connect_btn"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let mut locales = BTreeMap::new();
        locales.insert(
            "en".to_string(),
            make_locale(&["cancel_btn", "connect_btn"]),
        );
        locales.insert("de".to_string(), make_locale(&["cancel_btn"]));

        let violations = collect_violations(&source_keys, &locales, &[]);
        assert!(violations
            .iter()
            .any(|v| v.contains("connect_btn") && v.contains("de")));
    }

    #[test]
    fn detects_unused_key_in_locale() {
        let source_keys: BTreeSet<String> = ["cancel_btn"].iter().map(|s| s.to_string()).collect();
        let mut locales = BTreeMap::new();
        locales.insert("en".to_string(), make_locale(&["cancel_btn", "orphan_key"]));

        let violations = collect_violations(&source_keys, &locales, &[]);
        assert!(violations
            .iter()
            .any(|v| v.contains("orphan_key") && v.contains("unused")));
    }

    #[test]
    fn detects_hardcoded_string() {
        let source_keys: BTreeSet<String> = BTreeSet::new();
        let locales = BTreeMap::new();
        let hardcoded = vec![(
            "src/views/foo.rs".to_string(),
            "Choose a folder".to_string(),
        )];

        let violations = collect_violations(&source_keys, &locales, &hardcoded);
        assert!(violations.iter().any(|v| v.contains("Choose a folder")));
    }

    #[test]
    fn no_violations_when_clean() {
        let source_keys: BTreeSet<String> = ["cancel_btn"].iter().map(|s| s.to_string()).collect();
        let mut locales = BTreeMap::new();
        locales.insert("en".to_string(), make_locale(&["cancel_btn"]));

        let violations = collect_violations(&source_keys, &locales, &[]);
        assert!(violations.is_empty());
    }
}
