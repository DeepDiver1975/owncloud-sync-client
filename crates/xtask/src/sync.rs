use crate::check::{LOCALE_NAMES, LOCALES_DIR, SOURCE_DIR};
use crate::locale;
use crate::source_scan;
use std::path::Path;

pub fn run() -> anyhow::Result<()> {
    let src_dir = Path::new(SOURCE_DIR);
    let locales_dir = Path::new(LOCALES_DIR);

    let source_keys = source_scan::scan_source_keys(src_dir)?;

    for locale_name in LOCALE_NAMES {
        let path = locales_dir.join(format!("{}.yml", locale_name));
        let (map, raw) = locale::load_locale_file(&path)?;
        let locale_keys = locale::keys_for_locale(&map, locale_name);

        let mut missing: Vec<&str> = source_keys
            .iter()
            .filter(|k| !locale_keys.contains(k.as_str()))
            .map(|k| k.as_str())
            .collect();
        missing.sort();

        if missing.is_empty() {
            println!("{}.yml: up to date", locale_name);
            continue;
        }

        let updated = locale::append_stubs(&raw, locale_name, &missing);
        std::fs::write(&path, updated)?;
        println!("{}.yml: added {} stub(s): {}", locale_name, missing.len(), missing.join(", "));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::locale::append_stubs;

    #[test]
    fn append_stubs_adds_missing_keys() {
        let raw = "en:\n  cancel_btn: \"Cancel\"\n";
        let result = append_stubs(raw, "en", &["connect_btn", "back_btn"]);
        assert!(result.contains("connect_btn: \"\""));
        assert!(result.contains("back_btn: \"\""));
        assert!(result.contains("# new"));
        assert!(result.contains("cancel_btn: \"Cancel\""));
    }

    #[test]
    fn append_stubs_noop_when_empty() {
        let raw = "en:\n  cancel_btn: \"Cancel\"\n";
        let result = append_stubs(raw, "en", &[]);
        assert_eq!(result, raw);
    }
}
