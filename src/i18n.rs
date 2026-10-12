//! Picks the UI language at startup. Translations live in `locales/*.yml`
//! (English is the source language and the fallback for anything missing).

/// Environment variable that overrides the system language, e.g. `BT_BATTERY_LANG=en`.
const OVERRIDE_VAR: &str = "BT_BATTERY_LANG";

/// Selects the locale once at startup; the app does not follow later system language changes.
pub fn init() {
    let requested = std::env::var(OVERRIDE_VAR).ok().filter(|v| !v.is_empty()).or_else(sys_locale::get_locale);
    let locales = rust_i18n::available_locales!();
    let available: Vec<&str> = locales.iter().map(|l| l.as_ref()).collect();
    rust_i18n::set_locale(&resolve(requested.as_deref().unwrap_or("en"), &available));
}

/// Maps an OS locale tag to one of the shipped locales, falling back to English.
///
/// Accepts BCP 47 tags (`zh-Hans-CN`, `en-US`) and POSIX ones (`zh_CN.UTF-8`).
/// Chinese is split by script: Traditional (`Hant`, or the TW / HK / MO regions) → `zh-TW`,
/// everything else → `zh-CN`.
fn resolve(requested: &str, available: &[&str]) -> String {
    let tag = requested.split(['.', '@']).next().unwrap_or_default().replace('_', "-");
    let mut parts = tag.split('-').filter(|p| !p.is_empty());
    let lang = parts.next().unwrap_or_default().to_ascii_lowercase();
    let subtags: Vec<String> = parts.map(str::to_ascii_lowercase).collect();

    let mut candidates = Vec::new();
    if lang == "zh" {
        let traditional = subtags.iter().any(|s| matches!(s.as_str(), "hant" | "tw" | "hk" | "mo"));
        candidates.push(if traditional { "zh-TW".to_string() } else { "zh-CN".to_string() });
    } else if let Some(region) = subtags.iter().find(|s| s.len() == 2) {
        candidates.push(format!("{lang}-{}", region.to_ascii_uppercase()));
    }
    candidates.push(lang);

    candidates
        .into_iter()
        .find_map(|c| available.iter().find(|a| a.eq_ignore_ascii_case(&c)).map(|a| a.to_string()))
        .unwrap_or_else(|| "en".into())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::fs;

    use super::resolve;

    #[test]
    fn resolves_locales() {
        let available = ["en", "zh-CN"];
        assert_eq!(resolve("zh-Hans-CN", &available), "zh-CN");
        assert_eq!(resolve("zh_CN.UTF-8", &available), "zh-CN");
        assert_eq!(resolve("zh-SG", &available), "zh-CN");
        assert_eq!(resolve("en-US", &available), "en");
        assert_eq!(resolve("de-DE", &available), "en");
        assert_eq!(resolve("C", &available), "en");
        assert_eq!(resolve("", &available), "en");
        // No Traditional Chinese translation yet: fall back to English rather than Simplified
        assert_eq!(resolve("zh-Hant-TW", &available), "en");
        assert_eq!(resolve("zh-HK", &["en", "zh-CN", "zh-TW"]), "zh-TW");
    }

    #[test]
    fn translates_with_placeholders() {
        use rust_i18n::t;
        rust_i18n::set_locale("zh-CN");
        assert_eq!(t!("menu.quit"), "退出");
        assert_eq!(t!("notify.low.body", name = "NX87", level = 15), "NX87 电量仅剩 15%，请及时充电");
        rust_i18n::set_locale("en");
        assert_eq!(t!("status.charging", level = 40), "40% (charging)");
        assert_eq!(t!("notify.low.body", name = "NX87", level = 15), "NX87 is at 15%. Please charge it soon.");
    }

    /// `key → value` pairs of a flat locale file (`_version` and comments skipped)
    fn entries(file: &str) -> Vec<(String, String)> {
        let path = format!("{}/locales/{file}", env!("CARGO_MANIFEST_DIR"));
        fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("{path}: {e}"))
            .lines()
            .filter(|l| !l.trim_start().starts_with('#') && !l.starts_with("_version"))
            .filter_map(|l| l.split_once(':'))
            .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
            .collect()
    }

    fn placeholders(text: &str) -> BTreeSet<&str> {
        text.split("%{").skip(1).filter_map(|s| s.split_once('}')).map(|(name, _)| name).collect()
    }

    /// Every locale must have exactly the keys of en.yml, with the same placeholders.
    #[test]
    fn locales_match_english() {
        let english = entries("en.yml");
        let dir = format!("{}/locales", env!("CARGO_MANIFEST_DIR"));
        for file in fs::read_dir(dir).unwrap().flatten() {
            let name = file.file_name().into_string().unwrap();
            if name == "en.yml" {
                continue;
            }
            let translated = entries(&name);
            let keys = |e: &[(String, String)]| e.iter().map(|(k, _)| k.clone()).collect::<BTreeSet<_>>();
            assert_eq!(keys(&translated), keys(&english), "{name}: keys differ from en.yml");
            for (key, text) in &english {
                let other = &translated.iter().find(|(k, _)| k == key).unwrap().1;
                assert_eq!(placeholders(other), placeholders(text), "{name}: placeholders of `{key}`");
            }
        }
    }
}
