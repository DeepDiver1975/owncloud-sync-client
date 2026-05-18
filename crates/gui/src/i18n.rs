use crate::model::Language;

pub fn detect_system_language() -> Language {
    let tag = sys_locale::get_locale().unwrap_or_default();
    if tag.starts_with("de") {
        Language::De
    } else if tag.starts_with("fr") {
        Language::Fr
    } else if tag.starts_with("zh") {
        Language::Zh
    } else {
        Language::En
    }
}

pub fn translate_key_for_test(locale: &str, key: &str) -> String {
    use rust_i18n::t;
    use std::sync::{Mutex, OnceLock};

    static LOCALE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    let lock = LOCALE_LOCK.get_or_init(|| Mutex::new(()));
    let _guard = lock.lock().unwrap();
    let prev = rust_i18n::locale().to_string();
    rust_i18n::set_locale(locale);
    let result = t!(key).to_string();
    rust_i18n::set_locale(&prev);
    result
}
