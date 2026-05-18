use regex::Regex;
use std::collections::BTreeSet;
use std::path::Path;
use walkdir::WalkDir;

#[allow(dead_code)]
pub fn extract_t_keys(src: &str) -> Vec<String> {
    let re = Regex::new(r#"(^|[^a-zA-Z0-9_])t!\("([^"]+)""#).unwrap();
    let mut keys: BTreeSet<String> = BTreeSet::new();
    for cap in re.captures_iter(src) {
        keys.insert(cap[2].to_string());
    }
    keys.into_iter().collect()
}

#[allow(dead_code)]
pub fn scan_source_keys(src_dir: &Path) -> anyhow::Result<BTreeSet<String>> {
    let mut all_keys = BTreeSet::new();
    for entry in WalkDir::new(src_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map_or(false, |ext| ext == "rs"))
    {
        let src = std::fs::read_to_string(entry.path())?;
        for key in extract_t_keys(&src) {
            all_keys.insert(key);
        }
    }
    Ok(all_keys)
}

#[allow(dead_code)]
pub fn find_hardcoded_strings(src: &str) -> Vec<String> {
    let re = Regex::new(r#"text\("([^"]+)"\)"#).unwrap();
    let mut found = Vec::new();
    for line in src.lines() {
        if line.trim_end().ends_with("// i18n-ignore") {
            continue;
        }
        for cap in re.captures_iter(line) {
            let s = &cap[1];
            if s.starts_with("http://") || s.starts_with("https://") {
                continue;
            }
            if !s.chars().any(|c| c.is_ascii_alphabetic()) {
                continue;
            }
            found.push(s.to_string());
        }
    }
    found
}

#[allow(dead_code)]
pub fn scan_hardcoded_strings(src_dir: &Path) -> anyhow::Result<Vec<(String, String)>> {
    let mut results = Vec::new();
    for entry in WalkDir::new(src_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map_or(false, |ext| ext == "rs"))
    {
        let path = entry.path().to_string_lossy().to_string();
        let src = std::fs::read_to_string(entry.path())?;
        for s in find_hardcoded_strings(&src) {
            results.push((path.clone(), s));
        }
    }
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_simple_key() {
        let src = r#"let x = t!("cancel_btn").to_string();"#;
        let keys = extract_t_keys(src);
        assert_eq!(keys, vec!["cancel_btn"]);
    }

    #[test]
    fn extracts_key_with_named_args() {
        let src = r#"t!("folder_status_error_other", count = n)"#;
        let keys = extract_t_keys(src);
        assert_eq!(keys, vec!["folder_status_error_other"]);
    }

    #[test]
    fn extracts_multiple_keys() {
        let src = r#"
            let a = t!("key_one");
            let b = t!("key_two");
        "#;
        let mut keys = extract_t_keys(src);
        keys.sort();
        assert_eq!(keys, vec!["key_one", "key_two"]);
    }

    #[test]
    fn deduplicates_keys() {
        let src = r#"
            let a = t!("cancel_btn");
            let b = t!("cancel_btn");
        "#;
        let keys = extract_t_keys(src);
        assert_eq!(keys, vec!["cancel_btn"]);
    }

    #[test]
    fn does_not_extract_format_macro_strings() {
        let src = r#"text(format!("☁ {}", t!("nav_sync_status")))"#;
        let keys = extract_t_keys(src);
        assert_eq!(keys, vec!["nav_sync_status"]);
    }

    #[test]
    fn detects_hardcoded_text() {
        let src = r#"    let x = text("Choose a root folder");"#;
        let found = find_hardcoded_strings(src);
        assert_eq!(found, vec!["Choose a root folder"]);
    }

    #[test]
    fn skips_i18n_ignore() {
        let src = r#"    text("ownCloud Sync") // i18n-ignore"#;
        let found = find_hardcoded_strings(src);
        assert!(found.is_empty());
    }

    #[test]
    fn skips_url_strings() {
        let src = r#"    text("https://owncloud.com")"#;
        let found = find_hardcoded_strings(src);
        assert!(found.is_empty());
    }

    #[test]
    fn skips_single_symbol() {
        let src = r#"    text("→")"#;
        let found = find_hardcoded_strings(src);
        assert!(found.is_empty());
    }

    #[test]
    fn skips_pure_emoji() {
        let src = r#"    text("📁")"#;
        let found = find_hardcoded_strings(src);
        assert!(found.is_empty());
    }
}
