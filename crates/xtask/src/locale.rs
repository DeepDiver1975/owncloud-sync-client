use anyhow::Context;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub type LocaleMap = BTreeMap<String, BTreeMap<String, String>>;

pub fn parse_locale_yaml(yaml: &str) -> anyhow::Result<LocaleMap> {
    serde_yaml::from_str(yaml).context("failed to parse locale YAML")
}

pub fn keys_for_locale<'a>(locales: &'a LocaleMap, locale: &str) -> BTreeSet<&'a str> {
    locales
        .get(locale)
        .map(|m| m.keys().map(|k| k.as_str()).collect())
        .unwrap_or_default()
}

pub fn load_locale_file(path: &Path) -> anyhow::Result<(LocaleMap, String)> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let map = parse_locale_yaml(&raw)?;
    Ok((map, raw))
}

/// Append missing keys as empty stubs under a `# new` comment at the end of the locale block.
/// Preserves all existing content by operating on the raw string.
pub fn append_stubs(raw: &str, _locale: &str, missing_keys: &[&str]) -> String {
    if missing_keys.is_empty() {
        return raw.to_string();
    }
    let mut out = raw.trim_end_matches('\n').to_string();
    out.push('\n');
    out.push_str("\n  # new\n");
    for key in missing_keys {
        out.push_str(&format!("  {}: \"\"\n", key));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_locale_file() {
        let yaml = "en:\n  cancel_btn: \"Cancel\"\n  connect_btn: \"Connect\"\n";
        let locales = parse_locale_yaml(yaml).unwrap();
        assert_eq!(locales["en"]["cancel_btn"], "Cancel");
        assert_eq!(locales["en"]["connect_btn"], "Connect");
    }

    #[test]
    fn lists_keys_for_locale() {
        let yaml = "de:\n  cancel_btn: \"Abbrechen\"\n  connect_btn: \"Verbinden\"\n";
        let locales = parse_locale_yaml(yaml).unwrap();
        let keys = keys_for_locale(&locales, "de");
        assert!(keys.contains("cancel_btn"));
        assert!(keys.contains("connect_btn"));
    }
}
