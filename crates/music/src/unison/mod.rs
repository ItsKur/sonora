use std::time::Duration;

use anyhow::{Context as _, Result};
use async_trait::async_trait;
use serde::Deserialize;

use crate::lyrics::{lrc, ttml, undecorated};
use crate::{Lyrics, LyricsHit, LyricsProvider, LyricsQuery};

const SOURCE: &str = "Unison";
const BASE: &str = "https://unison.boidu.dev";
const YOUTUBE: &str = "youtube";
/// Above the apple catalogue, so a crowdsourced sheet wins a tie.
const TRUST_HIGH: u32 = 240;
const TRUST_MEDIUM: u32 = 210;
const TRUST_LOW: u32 = 190;
/// The search endpoint is fuzzy, so only a few plausible cards are worth fetching.
const CANDIDATES: usize = 3;
const TIMEOUT: Duration = Duration::from_secs(10);
const AGENT: &str = concat!(
    "sonora/",
    env!("CARGO_PKG_VERSION"),
    " (https://github.com/sonorahq/sonora)"
);

pub struct Unison {
    http: reqwest::Client,
}

impl Unison {
    pub fn new() -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(TIMEOUT)
                .build()
                .unwrap_or_default(),
        }
    }

    async fn ask<T: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        params: &[(&str, &str)],
    ) -> Result<Option<T>> {
        let response = self
            .http
            .get(format!("{BASE}{path}"))
            .query(params)
            .header("User-Agent", AGENT)
            .send()
            .await
            .context("cannot reach unison")?;
        let status = response.status();
        if status == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !status.is_success() {
            anyhow::bail!("unison answered with status {status}");
        }
        let answer: Answer<T> = response
            .json()
            .await
            .context("cannot read the unison response")?;
        Ok(answer.success.then_some(answer.data).flatten())
    }

    /// The exact route: unison is keyed by youtube video id, which is also our track id.
    async fn by_video(&self, video: &str) -> Result<Option<Entry>> {
        self.ask("/lyrics", &[("v", video)]).await
    }

    async fn by_metadata(&self, query: &LyricsQuery) -> Result<Option<Entry>> {
        self.ask(
            "/lyrics",
            &[
                ("song", query.title.as_str()),
                ("artist", query.artist.as_str()),
            ],
        )
        .await
    }

    async fn by_id(&self, id: u64) -> Result<Option<Entry>> {
        self.ask(&format!("/lyrics/{id}"), &[]).await
    }

    /// Full-text search returns cards without any lyrics, so plausible ones are fetched by id.
    async fn by_search(&self, query: &LyricsQuery) -> Result<Vec<Entry>> {
        let terms = format!("{} {}", query.title, query.artist);
        let cards: Vec<Card> = self
            .ask("/lyrics/search", &[("q", terms.trim())])
            .await?
            .unwrap_or_default();

        let mut entries = Vec::new();
        for card in cards
            .iter()
            .filter(|card| plausible(query, card))
            .take(CANDIDATES)
        {
            match self.by_id(card.id).await {
                Ok(Some(entry)) => entries.push(entry),
                Ok(None) => {}
                Err(error) => log::warn!("lyrics: cannot read unison entry {}: {error:#}", card.id),
            }
        }

        Ok(entries)
    }
}

impl Default for Unison {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Deserialize)]
struct Answer<T> {
    #[serde(default)]
    success: bool,
    #[serde(default = "none")]
    data: Option<T>,
}

fn none<T>() -> Option<T> {
    None
}

#[derive(Deserialize)]
struct Entry {
    #[serde(default)]
    song: String,
    #[serde(default)]
    artist: String,
    album: Option<String>,
    duration: Option<f64>,
    #[serde(default)]
    lyrics: String,
    #[serde(default)]
    format: String,
    #[serde(default)]
    confidence: String,
}

#[derive(Deserialize)]
struct Card {
    id: u64,
    #[serde(default)]
    song: String,
    #[serde(default)]
    artist: String,
}

impl Entry {
    fn trust(&self) -> u32 {
        match self.confidence.as_str() {
            "high" => TRUST_HIGH,
            "medium" => TRUST_MEDIUM,
            _ => TRUST_LOW,
        }
    }
}

/// Cheap pre-filter so the fuzzy search does not cost a fetch per unrelated card.
fn plausible(query: &LyricsQuery, card: &Card) -> bool {
    let (title, artist) = (undecorated(&card.song), undecorated(&card.artist));
    let (wanted, singer) = (undecorated(&query.title), undecorated(&query.artist));
    if title.is_empty() || wanted.is_empty() {
        return false;
    }
    let named = title.contains(&wanted) || wanted.contains(&title);
    let sung = artist.is_empty()
        || singer.is_empty()
        || artist.contains(&singer)
        || singer.contains(&artist);

    named && sung
}

/// Unison stores apple-flavoured ttml, lrc or plain text, and the format field can lie.
fn sheet(entry: &Entry) -> Option<(Lyrics, Vec<String>)> {
    let text = entry.lyrics.trim();
    if text.is_empty() {
        return None;
    }
    let xml = entry.format.eq_ignore_ascii_case("ttml") || text.starts_with('<');
    if xml
        && let Ok(parsed) = ttml::parse(text)
        && !parsed.lyrics.is_empty()
    {
        return Some((parsed.lyrics, parsed.writers));
    }

    let lines = lrc::parse(text);
    if !lines.is_empty() {
        return Some((
            Lyrics::Synced {
                lines: lines.into(),
            },
            Vec::new(),
        ));
    }

    (!xml).then(|| (Lyrics::plain(text), Vec::new()))
}

/// An entry reached by video id is an exact match, so the track's own tags describe it best.
fn hit(entry: Entry, query: Option<&LyricsQuery>) -> Option<LyricsHit> {
    let trust = entry.trust();
    let (lyrics, writers) = sheet(&entry)?;
    let duration = entry.duration.map(Duration::from_secs_f64);

    Some(match query {
        Some(query) => LyricsHit {
            source: SOURCE,
            trust,
            lyrics,
            instrumental: false,
            title: query.title.clone(),
            artist: query.artist.clone(),
            album: query.album.clone(),
            duration: duration.or((!query.duration.is_zero()).then_some(query.duration)),
            writers,
        },
        None => LyricsHit {
            source: SOURCE,
            trust,
            lyrics,
            instrumental: false,
            title: entry.song,
            artist: entry.artist,
            album: entry.album.filter(|album| !album.is_empty()),
            duration,
            writers,
        },
    })
}

#[async_trait]
impl LyricsProvider for Unison {
    fn name(&self) -> &'static str {
        SOURCE
    }

    async fn search(&self, query: &LyricsQuery) -> Result<Vec<LyricsHit>> {
        if let Some(video) = query.id_for(YOUTUBE)
            && let Some(found) = self.by_video(video).await?
            && let Some(hit) = hit(found, Some(query))
        {
            return Ok(vec![hit]);
        }

        if let Some(found) = self.by_metadata(query).await?
            && let Some(hit) = hit(found, None)
        {
            return Ok(vec![hit]);
        }

        Ok(self
            .by_search(query)
            .await?
            .into_iter()
            .filter_map(|entry| hit(entry, None))
            .collect())
    }
}
