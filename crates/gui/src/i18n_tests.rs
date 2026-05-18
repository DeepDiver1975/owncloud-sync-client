#[cfg(test)]
mod tests {
    use crate::i18n::translate_key_for_test;

    #[test]
    fn en_nav_sync_status_is_not_raw_key() {
        let val = translate_key_for_test("en", "nav_sync_status");
        assert_ne!(
            val, "nav_sync_status",
            "translation key was returned instead of translation — YAML not embedded at compile time"
        );
        assert_eq!(val, "Sync Status");
    }
}
