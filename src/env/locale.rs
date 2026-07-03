//! POSIX §8.2 locale resolution.
//!
//! yosh operates on C/POSIX locale semantics internally; non-C
//! locale values are preserved as variables and exported to child
//! processes unchanged, but yosh's internal pattern matching and
//! test comparisons interpret them as C. See
//! `docs/yosh/posix-compliance.md`.
//!
//! v1 scope: this module is the explicit branch point for a future
//! non-C extension. All current call sites fall back to C-locale
//! behaviour, so the public API here has no live consumers yet —
//! `#![allow(dead_code)]` is intentional.

#![allow(dead_code)]

use crate::env::ShellEnv;

/// POSIX locale categories.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum LocaleCategory {
    Collate,
    Ctype,
    Messages,
    Monetary,
    Numeric,
    Time,
}

impl LocaleCategory {
    fn env_var_name(self) -> &'static str {
        match self {
            LocaleCategory::Collate => "LC_COLLATE",
            LocaleCategory::Ctype => "LC_CTYPE",
            LocaleCategory::Messages => "LC_MESSAGES",
            LocaleCategory::Monetary => "LC_MONETARY",
            LocaleCategory::Numeric => "LC_NUMERIC",
            LocaleCategory::Time => "LC_TIME",
        }
    }
}

/// Which variable produced the resolved value.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum LocaleSource {
    LcAll,
    LcCategory,
    Lang,
    Default,
}

/// Resolved locale for a single category.
#[derive(Clone, Debug)]
pub struct ResolvedLocale {
    pub category: LocaleCategory,
    pub value: String,
    pub source: LocaleSource,
}

/// Resolve `category` per POSIX §8.2:
/// `LC_ALL` > `LC_<category>` > `LANG` > `"C"`.
///
/// Empty-string values are treated as "unset" per POSIX §8.2.
pub fn resolve(env: &ShellEnv, category: LocaleCategory) -> ResolvedLocale {
    if let Some(v) = env.vars.get("LC_ALL").filter(|s| !s.is_empty()) {
        return ResolvedLocale {
            category,
            value: v.to_string(),
            source: LocaleSource::LcAll,
        };
    }
    if let Some(v) = env
        .vars
        .get(category.env_var_name())
        .filter(|s| !s.is_empty())
    {
        return ResolvedLocale {
            category,
            value: v.to_string(),
            source: LocaleSource::LcCategory,
        };
    }
    if let Some(v) = env.vars.get("LANG").filter(|s| !s.is_empty()) {
        return ResolvedLocale {
            category,
            value: v.to_string(),
            source: LocaleSource::Lang,
        };
    }
    ResolvedLocale {
        category,
        value: "C".to_string(),
        source: LocaleSource::Default,
    }
}

/// True iff `value` names the POSIX (C-equivalent) locale.
///
/// POSIX XBD §7.2 specifies `"C"` and `"POSIX"` as the portable
/// locale names that produce identical behaviour. Empty string is
/// treated as "unset" by [`resolve`] and therefore never reaches
/// this predicate in normal use, but is accepted as `true` for
/// safety.
pub fn is_c_locale(value: &str) -> bool {
    value.is_empty() || value == "C" || value == "POSIX"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env::ShellEnv;

    fn empty_env() -> ShellEnv {
        // ShellEnv::new imports the host process environment via
        // VarStore::from_environ. Scrub locale variables so tests
        // start from a known-blank slate regardless of the
        // developer/CI shell's LANG / LC_*. Ignore unset errors:
        // locale variables are never readonly in normal use.
        let mut env = ShellEnv::new("yosh", vec![]);
        for v in &[
            "LC_ALL",
            "LC_COLLATE",
            "LC_CTYPE",
            "LC_MESSAGES",
            "LC_MONETARY",
            "LC_NUMERIC",
            "LC_TIME",
            "LANG",
        ] {
            let _ = env.vars.unset(v);
        }
        env
    }

    #[test]
    fn default_when_all_unset() {
        let env = empty_env();
        let r = resolve(&env, LocaleCategory::Collate);
        assert_eq!(r.value, "C");
        assert_eq!(r.source, LocaleSource::Default);
        assert_eq!(r.category, LocaleCategory::Collate);
    }

    #[test]
    fn lang_used_when_lc_unset() {
        let mut env = empty_env();
        env.assign_var("LANG", "en_US.UTF-8").unwrap();
        let r = resolve(&env, LocaleCategory::Ctype);
        assert_eq!(r.value, "en_US.UTF-8");
        assert_eq!(r.source, LocaleSource::Lang);
    }

    #[test]
    fn lc_category_overrides_lang() {
        let mut env = empty_env();
        env.assign_var("LANG", "en_US.UTF-8").unwrap();
        env.assign_var("LC_COLLATE", "C").unwrap();
        let r = resolve(&env, LocaleCategory::Collate);
        assert_eq!(r.value, "C");
        assert_eq!(r.source, LocaleSource::LcCategory);
    }

    #[test]
    fn lc_all_overrides_lc_category_and_lang() {
        let mut env = empty_env();
        env.assign_var("LANG", "en_US.UTF-8").unwrap();
        env.assign_var("LC_COLLATE", "fr_FR.UTF-8").unwrap();
        env.assign_var("LC_ALL", "C").unwrap();
        let r = resolve(&env, LocaleCategory::Collate);
        assert_eq!(r.value, "C");
        assert_eq!(r.source, LocaleSource::LcAll);
    }

    #[test]
    fn empty_lc_all_is_unset() {
        let mut env = empty_env();
        env.assign_var("LC_ALL", "").unwrap();
        env.assign_var("LC_COLLATE", "C").unwrap();
        let r = resolve(&env, LocaleCategory::Collate);
        // Empty LC_ALL must fall through to LC_COLLATE.
        assert_eq!(r.value, "C");
        assert_eq!(r.source, LocaleSource::LcCategory);
    }

    #[test]
    fn empty_lc_category_falls_through_to_lang() {
        let mut env = empty_env();
        env.assign_var("LANG", "en_US.UTF-8").unwrap();
        env.assign_var("LC_NUMERIC", "").unwrap();
        let r = resolve(&env, LocaleCategory::Numeric);
        assert_eq!(r.value, "en_US.UTF-8");
        assert_eq!(r.source, LocaleSource::Lang);
    }

    #[test]
    fn empty_lang_falls_through_to_default() {
        let mut env = empty_env();
        env.assign_var("LANG", "").unwrap();
        let r = resolve(&env, LocaleCategory::Messages);
        assert_eq!(r.value, "C");
        assert_eq!(r.source, LocaleSource::Default);
    }

    #[test]
    fn each_category_reads_its_own_var() {
        let mut env = empty_env();
        env.assign_var("LC_COLLATE", "v_collate").unwrap();
        env.assign_var("LC_CTYPE", "v_ctype").unwrap();
        env.assign_var("LC_MESSAGES", "v_msg").unwrap();
        env.assign_var("LC_MONETARY", "v_mon").unwrap();
        env.assign_var("LC_NUMERIC", "v_num").unwrap();
        env.assign_var("LC_TIME", "v_time").unwrap();
        assert_eq!(resolve(&env, LocaleCategory::Collate).value, "v_collate");
        assert_eq!(resolve(&env, LocaleCategory::Ctype).value, "v_ctype");
        assert_eq!(resolve(&env, LocaleCategory::Messages).value, "v_msg");
        assert_eq!(resolve(&env, LocaleCategory::Monetary).value, "v_mon");
        assert_eq!(resolve(&env, LocaleCategory::Numeric).value, "v_num");
        assert_eq!(resolve(&env, LocaleCategory::Time).value, "v_time");
    }

    #[test]
    fn is_c_locale_recognizes_portable_names() {
        assert!(is_c_locale("C"));
        assert!(is_c_locale("POSIX"));
        assert!(is_c_locale(""));
    }

    #[test]
    fn is_c_locale_rejects_others() {
        assert!(!is_c_locale("en_US.UTF-8"));
        assert!(!is_c_locale("ja_JP.UTF-8"));
        assert!(!is_c_locale("c")); // case-sensitive per POSIX
    }
}
