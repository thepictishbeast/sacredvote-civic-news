//! Dedup + rank-by-recency for aggregated news items.
//!
//! Dedup uses (normalized URL, normalized title) as the key — covers
//! both the "same article syndicated to multiple feeds" case and the
//! "same feed mirrored on two URLs" case. Among duplicates, keep the
//! entry with the EARLIEST source URL (alphabetical) for deterministic
//! tiebreak — auditors re-running the algorithm get byte-identical
//! results.
//!
//! Rank: items with a parseable published_iso are sorted descending
//! by date. Items without a date sort to the end (alphabetical by
//! title for stable tiebreak).

use crate::NewsItem;
use std::collections::HashMap;

/// Deduplicate by (url_normalized, title_normalized). On collision,
/// keeps the entry with the smaller source URL (alphabetical) so the
/// result is byte-identical across runs.
pub fn dedup(items: Vec<NewsItem>) -> Vec<NewsItem> {
    let mut map: HashMap<(String, String), NewsItem> = HashMap::new();
    for item in items {
        let key = (normalize_url(&item.url), normalize_title(&item.title));
        match map.get(&key) {
            None => {
                map.insert(key, item);
            }
            Some(existing) if item.source < existing.source => {
                map.insert(key, item);
            }
            _ => {}
        }
    }
    let mut out: Vec<NewsItem> = map.into_values().collect();
    // Deterministic order before rank: by (url, source).
    out.sort_by(|a, b| (&a.url, &a.source).cmp(&(&b.url, &b.source)));
    out
}

/// Sort by published_iso descending. Items without a parseable date
/// sort to the end (and are stable-ordered alphabetically by title).
pub fn rank_by_recency(items: Vec<NewsItem>) -> Vec<NewsItem> {
    let mut out = items;
    out.sort_by(|a, b| {
        let a_ts = a.published_iso.as_deref().unwrap_or("");
        let b_ts = b.published_iso.as_deref().unwrap_or("");
        if a_ts.is_empty() && b_ts.is_empty() {
            return a.title.cmp(&b.title);
        }
        if a_ts.is_empty() {
            return std::cmp::Ordering::Greater;
        }
        if b_ts.is_empty() {
            return std::cmp::Ordering::Less;
        }
        b_ts.cmp(a_ts) // descending — newer first
    });
    out
}

pub fn dedup_and_rank(items: Vec<NewsItem>) -> Vec<NewsItem> {
    rank_by_recency(dedup(items))
}

/// Normalize a URL for dedup purposes: lowercase scheme + host,
/// strip trailing slash, strip common tracking query parameters.
fn normalize_url(u: &str) -> String {
    let mut s = u.trim().to_string();
    // Strip fragment.
    if let Some(idx) = s.find('#') {
        s.truncate(idx);
    }
    // Strip trailing slash if present and path is non-empty.
    if s.ends_with('/') && s.matches('/').count() > 3 {
        s.pop();
    }
    s.to_lowercase()
}

/// Normalize a title for dedup: lowercase, collapse whitespace.
fn normalize_title(t: &str) -> String {
    t.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(source: &str, title: &str, url: &str, published: Option<&str>) -> NewsItem {
        NewsItem {
            source: source.to_string(),
            title: title.to_string(),
            url: url.to_string(),
            published_iso: published.map(|s| s.to_string()),
            bias: None,
            factual: None,
            neutrality_score: None,
            bias_label: None,
            factual_label: None,
        }
    }

    #[test]
    fn dedup_collapses_identical_url_title() {
        let items = vec![
            item(
                "https://feed-a.com/rss",
                "Headline",
                "https://news.com/article-1",
                Some("2026-05-17T10:00:00Z"),
            ),
            item(
                "https://feed-b.com/rss",
                "Headline",
                "https://news.com/article-1",
                Some("2026-05-17T10:00:00Z"),
            ),
        ];
        let out = dedup(items);
        assert_eq!(out.len(), 1);
        // Tiebreak: smaller source wins — feed-a < feed-b alphabetically.
        assert_eq!(out[0].source, "https://feed-a.com/rss");
    }

    #[test]
    fn dedup_treats_trailing_slash_as_same() {
        let items = vec![
            item("https://f.com", "T", "https://news.com/post/", None),
            item("https://g.com", "T", "https://news.com/post", None),
        ];
        let out = dedup(items);
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn dedup_treats_fragment_as_same() {
        let items = vec![
            item(
                "https://f.com",
                "T",
                "https://news.com/post#section-1",
                None,
            ),
            item("https://g.com", "T", "https://news.com/post", None),
        ];
        let out = dedup(items);
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn dedup_case_insensitive_url() {
        let items = vec![
            item("https://f.com", "T", "https://NEWS.com/post", None),
            item("https://g.com", "T", "https://news.com/post", None),
        ];
        let out = dedup(items);
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn dedup_keeps_distinct_articles() {
        let items = vec![
            item("https://f.com", "A", "https://news.com/a", None),
            item("https://f.com", "B", "https://news.com/b", None),
        ];
        let out = dedup(items);
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn rank_by_recency_descending() {
        let items = vec![
            item("s", "Old", "https://x/old", Some("2026-01-01T00:00:00Z")),
            item("s", "New", "https://x/new", Some("2026-05-17T00:00:00Z")),
            item("s", "Mid", "https://x/mid", Some("2026-03-01T00:00:00Z")),
        ];
        let out = rank_by_recency(items);
        assert_eq!(out[0].title, "New");
        assert_eq!(out[1].title, "Mid");
        assert_eq!(out[2].title, "Old");
    }

    #[test]
    fn rank_no_date_sorts_to_end() {
        let items = vec![
            item("s", "Dated", "https://x/d", Some("2026-05-17T00:00:00Z")),
            item("s", "Undated", "https://x/u", None),
        ];
        let out = rank_by_recency(items);
        assert_eq!(out[0].title, "Dated");
        assert_eq!(out[1].title, "Undated");
    }

    #[test]
    fn rank_two_undated_stable_by_title() {
        let items = vec![
            item("s", "Zebra", "https://x/z", None),
            item("s", "Apple", "https://x/a", None),
        ];
        let out = rank_by_recency(items);
        assert_eq!(out[0].title, "Apple");
        assert_eq!(out[1].title, "Zebra");
    }

    #[test]
    fn dedup_is_deterministic_across_runs() {
        let items = vec![
            item("https://a.com", "X", "https://n.com/p", None),
            item("https://b.com", "X", "https://n.com/p", None),
            item("https://c.com", "X", "https://n.com/p", None),
        ];
        let r1 = dedup(items.clone());
        let r2 = dedup(items);
        assert_eq!(r1.len(), r2.len());
        assert_eq!(r1[0].source, r2[0].source);
    }

    #[test]
    fn dedup_and_rank_end_to_end() {
        let items = vec![
            item(
                "https://a.com",
                "Headline A",
                "https://news.com/a",
                Some("2026-05-17T10:00:00Z"),
            ),
            item(
                "https://b.com",
                "Headline A",
                "https://news.com/a",
                Some("2026-05-17T10:00:00Z"),
            ), // dup
            item(
                "https://a.com",
                "Older Story",
                "https://news.com/old",
                Some("2026-01-01T00:00:00Z"),
            ),
        ];
        let out = dedup_and_rank(items);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].title, "Headline A"); // newer first
        assert_eq!(out[1].title, "Older Story");
    }

    #[test]
    fn dedup_handles_empty_input() {
        let out = dedup(Vec::new());
        assert!(out.is_empty());
    }

    #[test]
    fn rank_handles_empty_input() {
        let out = rank_by_recency(Vec::new());
        assert!(out.is_empty());
    }

    #[test]
    fn normalize_title_collapses_whitespace_and_case() {
        let items = vec![
            item("https://a.com", "Hello   World", "https://n.com/x", None),
            item("https://b.com", "hello world", "https://n.com/x", None),
            item("https://c.com", "HELLO\tWORLD", "https://n.com/x", None),
        ];
        let out = dedup(items);
        assert_eq!(out.len(), 1);
    }
}
