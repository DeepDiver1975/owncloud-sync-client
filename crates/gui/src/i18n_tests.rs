#[cfg(test)]
mod tests {
    use crate::i18n::translate_key_for_test;

    // ── Group 1: key-not-returned-as-self ────────────────────────────────────
    // Direct guard against the original symptom: raw key shown in GUI.

    #[test]
    fn en_nav_sync_status_is_not_raw_key() {
        let val = translate_key_for_test("en", "nav_sync_status");
        assert_ne!(
            val, "nav_sync_status",
            "translation key was returned instead of translation — YAML not embedded at compile time"
        );
        assert_eq!(val, "Sync Status");
    }

    // ── Group 2: spot-check keys across all 4 locales ────────────────────────

    #[test]
    fn nav_sync_status_translated_in_all_locales() {
        let cases = [
            ("en", "Sync Status"),
            ("de", "Synchronisierungsstatus"),
            ("fr", "État de synchronisation"),
            ("zh", "同步状态"),
        ];
        for (locale, expected) in cases {
            assert_eq!(
                translate_key_for_test(locale, "nav_sync_status"),
                expected,
                "locale={locale}"
            );
        }
    }

    #[test]
    fn cancel_btn_translated_in_all_locales() {
        let cases = [
            ("en", "Cancel"),
            ("de", "Abbrechen"),
            ("fr", "Annuler"),
            ("zh", "取消"),
        ];
        for (locale, expected) in cases {
            assert_eq!(
                translate_key_for_test(locale, "cancel_btn"),
                expected,
                "locale={locale}"
            );
        }
    }

    // ── Group 3: pick_root_folder keys ───────────────────────────────────────

    #[test]
    fn pick_root_folder_heading_translated_in_all_locales() {
        let cases = [
            ("en", "Choose a root folder"),
            ("de", "Stammordner auswählen"),
            ("fr", "Choisir un dossier racine"),
            ("zh", "选择根文件夹"),
        ];
        for (locale, expected) in cases {
            assert_eq!(
                translate_key_for_test(locale, "pick_root_folder_heading"),
                expected,
                "locale={locale}"
            );
        }
    }

    #[test]
    fn pick_root_folder_caption_translated_in_all_locales() {
        let cases = [
            ("en", "All selected spaces will sync as sub-folders inside this folder."),
            ("de", "Alle ausgewählten Spaces werden als Unterordner in diesem Ordner synchronisiert."),
            ("fr", "Tous les espaces sélectionnés seront synchronisés comme sous-dossiers dans ce dossier."),
            ("zh", "所有选定的空间将作为子文件夹同步到此文件夹中。"),
        ];
        for (locale, expected) in cases {
            assert_eq!(
                translate_key_for_test(locale, "pick_root_folder_caption"),
                expected,
                "locale={locale}"
            );
        }
    }

    #[test]
    fn root_folder_label_translated_in_all_locales() {
        let cases = [
            ("en", "Root folder"),
            ("de", "Stammordner"),
            ("fr", "Dossier racine"),
            ("zh", "根文件夹"),
        ];
        for (locale, expected) in cases {
            assert_eq!(
                translate_key_for_test(locale, "root_folder_label"),
                expected,
                "locale={locale}"
            );
        }
    }

    #[test]
    fn will_create_label_translated_in_all_locales() {
        let cases = [
            ("en", "Will create:"),
            ("de", "Wird erstellt:"),
            ("fr", "Créera :"),
            ("zh", "将创建："),
        ];
        for (locale, expected) in cases {
            assert_eq!(
                translate_key_for_test(locale, "will_create_label"),
                expected,
                "locale={locale}"
            );
        }
    }

    // ── Group 4: fallback behaviour ──────────────────────────────────────────

    #[test]
    fn unknown_locale_falls_back_to_english() {
        // "xx" is not in any YAML; fallback = "en" must kick in.
        let val = translate_key_for_test("xx", "nav_sync_status");
        assert_eq!(
            val, "Sync Status",
            "fallback to 'en' did not work for unknown locale 'xx'"
        );
    }

    // ── Group 5: no key equals its own name ──────────────────────────────────

    #[test]
    fn no_translation_equals_its_own_key() {
        // If the backend is not loaded, t!() returns the key literal itself.
        // This test fails immediately in that scenario.
        let spot_checks = [
            "nav_sync_status",
            "nav_settings",
            "add_account_heading",
            "cancel_btn",
            "pick_root_folder_heading",
            "root_folder_label",
        ];
        for key in spot_checks {
            let val = translate_key_for_test("en", key);
            assert_ne!(
                val, key,
                "key '{key}' returned itself — translation backend not loaded"
            );
        }
    }
}
