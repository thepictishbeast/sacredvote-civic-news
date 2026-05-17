//! Feed fetching + parsing.
//!
//! Fetches RSS / Atom / JSON Feed via reqwest with rustls-tls, parses
//! via feed-rs, normalizes to NewsItem. HTTPS-only URLs, bounded
//! response size, bounded fetch timeout.
//!
//! Design pins:
//!   - HTTPS-ONLY-SOURCE-URLS (defense against intranet SSRF probing)
//!   - RESPONSE-SIZE-CAPPED (defeats memory-exhaustion on hostile feeds)
//!   - FETCH-TIMEOUT-BOUNDED (defeats slowloris-style stalls)
//!   - PARSE-ERRORS-DROP-FEED-NOT-CRASH (one bad feed != all bad)
//!   - DETERMINISTIC-FROM-SAME-INPUT (no time-of-day randomness)

use crate::NewsItem;
use anyhow::{anyhow, Context, Result};
use std::time::Duration;

pub const MAX_RESPONSE_BYTES: u64 = 8 * 1024 * 1024; // 8 MB cap per feed
pub const FETCH_TIMEOUT_SECS: u64 = 10;
pub const MAX_ITEMS_PER_FEED: usize = 200;

/// Build a reqwest client configured for sidecar use: HTTPS-only via
/// rustls, bounded timeout, no redirects to unrelated hosts beyond a
/// small budget, ASCII user-agent.
pub fn build_client() -> Result<reqwest::Client> {
    let ua = format!(
        "{}/{} (+https://sacred.vote)",
        crate::SERVICE_NAME,
        crate::SERVICE_VERSION
    );
    reqwest::Client::builder()
        .user_agent(ua)
        .timeout(Duration::from_secs(FETCH_TIMEOUT_SECS))
        .redirect(reqwest::redirect::Policy::limited(3))
        .https_only(true)
        .build()
        .context("build reqwest client")
}

/// Fetch + parse a single feed URL. Returns Vec<NewsItem> on success,
/// or an error on network/parse failure. Caller decides whether one
/// bad feed should fail the whole batch (typically: no — just skip).
pub async fn fetch_and_parse(client: &reqwest::Client, url: &str) -> Result<Vec<NewsItem>> {
    if !url.starts_with("https://") {
        return Err(anyhow!("fetch_and_parse: HTTPS-only; got: {}", url));
    }
    let resp = client.get(url).send().await.context("send request")?;
    let status = resp.status();
    if !status.is_success() {
        return Err(anyhow!("fetch_and_parse: status {} for {}", status, url));
    }
    // Bounded read — defeat a hostile feed sending an infinite stream.
    let bytes = read_capped(resp, MAX_RESPONSE_BYTES).await?;
    parse_bytes(&bytes, url)
}

async fn read_capped(resp: reqwest::Response, cap: u64) -> Result<bytes::Bytes> {
    use futures::StreamExt;
    let mut buf = Vec::new();
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("read body chunk")?;
        if (buf.len() as u64) + (chunk.len() as u64) > cap {
            return Err(anyhow!(
                "read_capped: response exceeds {} bytes",
                cap
            ));
        }
        buf.extend_from_slice(&chunk);
    }
    Ok(bytes::Bytes::from(buf))
}

/// Parse a byte buffer (HTML/XML/JSON-Feed) as a syndication feed.
/// Returns up to MAX_ITEMS_PER_FEED items. Synthetic / source URL is
/// stored on each item for downstream attribution.
pub fn parse_bytes(bytes: &[u8], source_url: &str) -> Result<Vec<NewsItem>> {
    let parsed =
        feed_rs::parser::parse(bytes).context("feed_rs::parser::parse")?;
    let mut out = Vec::with_capacity(parsed.entries.len().min(MAX_ITEMS_PER_FEED));
    for entry in parsed.entries.iter().take(MAX_ITEMS_PER_FEED) {
        let title = entry
            .title
            .as_ref()
            .map(|t| sanitize_text(&t.content))
            .unwrap_or_default();
        // Pick the first link with rel=alternate or just the first link.
        let link = entry
            .links
            .iter()
            .find(|l| l.rel.as_deref() == Some("alternate"))
            .or_else(|| entry.links.first())
            .map(|l| l.href.clone())
            .unwrap_or_default();
        if title.is_empty() || link.is_empty() {
            continue;
        }
        if !link.starts_with("https://") && !link.starts_with("http://") {
            // skip mailto: + javascript: + relative
            continue;
        }
        let published_iso = entry
            .published
            .or(entry.updated)
            .map(|t| t.to_rfc3339());
        out.push(NewsItem {
            source: source_url.to_string(),
            title,
            url: link,
            published_iso,
            bias: None,
            factual: None,
            neutrality_score: None,
            bias_label: None,
        });
    }
    Ok(out)
}

/// Sanitize free-text title fields: strip control chars, collapse
/// whitespace, cap at 300 chars.
pub fn sanitize_text(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .filter(|c| !c.is_control() || *c == '\t' || *c == '\n')
        .collect();
    let collapsed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out = collapsed.trim().to_string();
    if out.chars().count() > 300 {
        out = out.chars().take(300).collect::<String>();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn https_only_rejects_http() {
        // Build a client, but we test the URL-check path without making a request.
        let client = build_client().unwrap();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(fetch_and_parse(&client, "http://example.com/rss"));
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("HTTPS-only"));
    }

    #[test]
    fn sanitize_text_strips_control_chars() {
        let raw = "hello\x00\x01world\x07!";
        let clean = sanitize_text(raw);
        assert_eq!(clean, "helloworld!");
    }

    #[test]
    fn sanitize_text_collapses_whitespace() {
        let raw = "hello\n\n   world   foo   ";
        let clean = sanitize_text(raw);
        assert_eq!(clean, "hello world foo");
    }

    #[test]
    fn sanitize_text_caps_at_300_chars() {
        let raw = "x".repeat(500);
        let clean = sanitize_text(&raw);
        assert_eq!(clean.chars().count(), 300);
    }

    #[test]
    fn sanitize_text_preserves_unicode() {
        let raw = "Hola — bienvenido";
        let clean = sanitize_text(raw);
        assert_eq!(clean, "Hola — bienvenido");
    }

    #[test]
    fn parse_bytes_handles_minimal_rss() {
        // Synthetic minimal RSS 2.0 feed.
        let rss = br#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0">
  <channel>
    <title>Test Feed</title>
    <link>https://example.com/</link>
    <description>Test</description>
    <item>
      <title>Headline One</title>
      <link>https://example.com/post-1</link>
      <pubDate>Wed, 17 May 2026 12:00:00 +0000</pubDate>
    </item>
    <item>
      <title>Headline Two</title>
      <link>https://example.com/post-2</link>
      <pubDate>Tue, 16 May 2026 09:00:00 +0000</pubDate>
    </item>
  </channel>
</rss>"#;
        let items = parse_bytes(rss, "https://example.com/feed").unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].title, "Headline One");
        assert_eq!(items[0].url, "https://example.com/post-1");
        assert!(items[0].published_iso.is_some());
        assert_eq!(items[0].source, "https://example.com/feed");
    }

    #[test]
    fn parse_bytes_drops_entries_without_title_or_link() {
        let rss = br#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0">
  <channel>
    <title>Test</title>
    <link>https://example.com/</link>
    <description>Test</description>
    <item><title>Has title but no link</title></item>
    <item><link>https://example.com/no-title</link></item>
    <item><title>Good</title><link>https://example.com/ok</link></item>
  </channel>
</rss>"#;
        let items = parse_bytes(rss, "https://example.com/feed").unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "Good");
    }

    #[test]
    fn parse_bytes_rejects_non_http_links() {
        let rss = br#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0">
  <channel>
    <title>Test</title>
    <link>https://example.com/</link>
    <description>Test</description>
    <item><title>JS Trap</title><link>javascript:alert(1)</link></item>
    <item><title>Mailto</title><link>mailto:bad@example.com</link></item>
    <item><title>Good</title><link>https://example.com/ok</link></item>
  </channel>
</rss>"#;
        let items = parse_bytes(rss, "https://example.com/feed").unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "Good");
    }

    #[test]
    fn parse_bytes_caps_at_max_items() {
        let mut rss = String::from(r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0"><channel><title>T</title><link>https://e.com/</link><description>T</description>"#);
        for i in 0..(MAX_ITEMS_PER_FEED + 50) {
            rss.push_str(&format!(
                r#"<item><title>Item {i}</title><link>https://e.com/{i}</link></item>"#
            ));
        }
        rss.push_str("</channel></rss>");
        let items = parse_bytes(rss.as_bytes(), "https://e.com/feed").unwrap();
        assert_eq!(items.len(), MAX_ITEMS_PER_FEED);
    }

    #[test]
    fn parse_bytes_atom_feed() {
        let atom = br#"<?xml version="1.0" encoding="utf-8"?>
<feed xmlns="http://www.w3.org/2005/Atom">
  <title>Test Atom</title>
  <id>https://example.com/atom</id>
  <updated>2026-05-17T12:00:00Z</updated>
  <entry>
    <title>Atom Headline</title>
    <id>https://example.com/atom-1</id>
    <updated>2026-05-17T12:00:00Z</updated>
    <link href="https://example.com/atom-1" />
  </entry>
</feed>"#;
        let items = parse_bytes(atom, "https://example.com/atom").unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "Atom Headline");
    }

    #[test]
    fn parse_bytes_malformed_returns_err() {
        let r = parse_bytes(b"not a feed", "https://x");
        assert!(r.is_err());
    }

    #[test]
    fn build_client_succeeds() {
        let r = build_client();
        assert!(r.is_ok());
    }
}
