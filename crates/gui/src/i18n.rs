rust_i18n::i18n!(
    "locales",
    fallback = "en"
);

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
