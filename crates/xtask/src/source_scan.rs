use regex::Regex;
use std::collections::BTreeSet;
use std::path::Path;
use walkdir::WalkDir;

#[allow(dead_code)]
pub fn extract_t_keys(src: &str) -> Vec<String> {
    let re = Regex::new(r#"t!\("([^"]+)""#).unwrap();
    let mut keys: BTreeSet<String> = BTreeSet::new();
    for cap in re.captures_iter(src) {
        keys.insert(cap[1].to_string());
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
}
