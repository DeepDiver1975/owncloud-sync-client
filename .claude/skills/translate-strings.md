---
name: translate-strings
description: Translate empty translation stubs in locale YAML files using en.yml as source of truth
---

# Translate Strings

Translate all empty (`""`) values in the locale YAML files using `en.yml` as the source of truth.

## Steps

1. Read `crates/gui/locales/en.yml` to understand all available strings and their context (use the `# section` comments).

2. For each locale file (`de.yml`, `fr.yml`, `zh.yml`):
   a. Read the file.
   b. Find all keys with empty values (`key: ""`).
   c. For each empty key, look up the English value in `en.yml`.
   d. Translate the English value into the target language, maintaining the tone and style of existing translations in that file.
   e. Preserve any `%{variable}` interpolation placeholders exactly as they appear in the English string.
   f. Write the completed translations back using Edit.

3. Run `just check-translations` to verify no violations remain.

4. Report: list each key translated per locale, and flag any keys you were uncertain about.

## Notes

- Do NOT translate: product names (`ownCloud`), URLs, placeholder variables (`%{count}`, `%{folder}`, `%{server}`)
- Match formality/tone of existing translations in each locale
- If a key is clearly a button label, keep it short and imperative
- `de.yml`: German (informal `du` register, matching existing translations)
- `fr.yml`: French (informal `tu` register, matching existing translations)
- `zh.yml`: Simplified Chinese
