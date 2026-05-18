//! Per-source bias + factual-tier registry, loaded from a TOML file.
//!
//! The registry is OPERATIONAL data (which feed URL → what rating)
//! and is intentionally separate from the algebra in neutrality.rs.
//! Sacred.Vote operators can swap the file without recompiling.
//!
//! Wire format (TOML):
//!
//! ```toml
//! [[source]]
//! url = "https://example.com/rss"
//! bias = "center"
//! factual = "high"
//!
//! [[source]]
//! url = "https://another.example/atom"
//! bias = "center-right"
//! factual = "very-high"
//! ```
//!
//! Lookups normalize URLs the same way `dedup_rank` does, so trailing
//! slashes and fragments do not cause misses.
//!
//! Design pins:
//!   - REGISTRY-IS-OPT-IN — missing TOML or unparseable TOML yields an
//!     empty registry (sources annotated as Unknown/Unknown).
//!   - URLS-NORMALIZED-FOR-LOOKUP — case-insensitive, trailing slash
//!     stripped, fragment stripped (matches dedup behavior).
//!   - RENDER-PATH-NO-PANIC — load_from_path returns Result; consumer
//!     code falls back to empty registry on error.

use crate::neutrality::{BiasRating, FactualTier};
use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Deserialize)]
struct RawConfig {
    #[serde(default, rename = "source")]
    sources: Vec<RawSource>,
}

#[derive(Debug, Deserialize)]
struct RawSource {
    url: String,
    bias: BiasRating,
    factual: FactualTier,
}

#[derive(Debug, Clone, Default)]
pub struct Registry {
    by_url: HashMap<String, (BiasRating, FactualTier)>,
}

impl Registry {
    pub fn empty() -> Self {
        Registry {
            by_url: HashMap::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.by_url.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_url.is_empty()
    }

    /// Look up the rating for a source URL. Returns (Unknown, Unknown)
    /// for misses so the consumer never has to special-case None.
    pub fn lookup(&self, url: &str) -> (BiasRating, FactualTier) {
        let key = normalize_for_lookup(url);
        self.by_url
            .get(&key)
            .copied()
            .unwrap_or((BiasRating::Unknown, FactualTier::Unknown))
    }

    pub fn load_from_path<P: AsRef<Path>>(path: P) -> Result<Self> {
        let contents = std::fs::read_to_string(path.as_ref())
            .with_context(|| format!("read TOML at {:?}", path.as_ref()))?;
        Self::load_from_str(&contents)
    }

    pub fn load_from_str(s: &str) -> Result<Self> {
        let parsed: RawConfig = toml::from_str(s).context("parse TOML")?;
        let mut by_url = HashMap::new();
        for src in parsed.sources {
            by_url.insert(normalize_for_lookup(&src.url), (src.bias, src.factual));
        }
        Ok(Registry { by_url })
    }

    /// Load from the path in CIVIC_NEWS_RATINGS_TOML env var, OR
    /// return an empty registry. Never panics; logs a warning on
    /// parse error.
    pub fn load_from_env_or_empty() -> Self {
        let path = match std::env::var("CIVIC_NEWS_RATINGS_TOML") {
            Ok(p) => p,
            Err(_) => return Self::empty(),
        };
        match Self::load_from_path(&path) {
            Ok(r) => {
                tracing::info!(path = %path, count = r.len(), "loaded source registry");
                r
            }
            Err(e) => {
                tracing::warn!(path = %path, error = %e, "failed to load source registry; using empty");
                Self::empty()
            }
        }
    }
}

fn normalize_for_lookup(u: &str) -> String {
    let mut s = u.trim().to_string();
    if let Some(idx) = s.find('#') {
        s.truncate(idx);
    }
    if s.ends_with('/') && s.matches('/').count() > 3 {
        s.pop();
    }
    s.to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::neutrality::{BiasRating, FactualTier};

    const FIXTURE: &str = r#"
[[source]]
url = "https://example.com/rss"
bias = "center"
factual = "high"

[[source]]
url = "https://another.example/atom"
bias = "center-right"
factual = "very-high"
"#;

    #[test]
    fn empty_registry_is_empty() {
        let r = Registry::empty();
        assert_eq!(r.len(), 0);
        assert!(r.is_empty());
    }

    #[test]
    fn empty_registry_returns_unknown() {
        let r = Registry::empty();
        assert_eq!(
            r.lookup("https://anything"),
            (BiasRating::Unknown, FactualTier::Unknown)
        );
    }

    #[test]
    fn load_from_str_parses_two_sources() {
        let r = Registry::load_from_str(FIXTURE).unwrap();
        assert_eq!(r.len(), 2);
    }

    #[test]
    fn lookup_exact_match() {
        let r = Registry::load_from_str(FIXTURE).unwrap();
        assert_eq!(
            r.lookup("https://example.com/rss"),
            (BiasRating::Center, FactualTier::High)
        );
        assert_eq!(
            r.lookup("https://another.example/atom"),
            (BiasRating::CenterRight, FactualTier::VeryHigh)
        );
    }

    #[test]
    fn lookup_case_insensitive() {
        let r = Registry::load_from_str(FIXTURE).unwrap();
        assert_eq!(
            r.lookup("https://EXAMPLE.com/RSS"),
            (BiasRating::Center, FactualTier::High)
        );
    }

    #[test]
    fn lookup_trailing_slash_normalized() {
        let toml = r#"[[source]]
url = "https://example.com/rss/"
bias = "center"
factual = "high"
"#;
        let r = Registry::load_from_str(toml).unwrap();
        // The stored URL had a trailing slash, but normalize strips it.
        // So lookup for both forms should match.
        assert_eq!(r.lookup("https://example.com/rss/").0, BiasRating::Center);
        assert_eq!(r.lookup("https://example.com/rss").0, BiasRating::Center);
    }

    #[test]
    fn lookup_fragment_stripped() {
        let r = Registry::load_from_str(FIXTURE).unwrap();
        assert_eq!(
            r.lookup("https://example.com/rss#fragment"),
            (BiasRating::Center, FactualTier::High)
        );
    }

    #[test]
    fn lookup_unknown_url_returns_unknown_unknown() {
        let r = Registry::load_from_str(FIXTURE).unwrap();
        assert_eq!(
            r.lookup("https://nope.example/feed"),
            (BiasRating::Unknown, FactualTier::Unknown)
        );
    }

    #[test]
    fn load_from_str_rejects_bad_toml() {
        let r = Registry::load_from_str("not [valid] toml = at all");
        assert!(r.is_err());
    }

    #[test]
    fn load_from_str_rejects_bad_enum_value() {
        let bad = r#"[[source]]
url = "https://x"
bias = "ultra-mega-left"
factual = "high"
"#;
        let r = Registry::load_from_str(bad);
        assert!(r.is_err());
    }

    #[test]
    fn load_from_str_empty_yields_empty() {
        let r = Registry::load_from_str("").unwrap();
        assert!(r.is_empty());
    }

    #[test]
    fn load_from_env_or_empty_returns_empty_when_unset() {
        std::env::remove_var("CIVIC_NEWS_RATINGS_TOML");
        let r = Registry::load_from_env_or_empty();
        assert!(r.is_empty());
    }
}
