# Unison lyrics

This fork adds [Unison](https://github.com/better-lyrics/unison) — the crowdsourced lyrics
API behind the Better Lyrics browser extension — as a lyrics provider, registered in
`crates/sonora/src/main.rs` ahead of the existing ones.

Unison mostly carries Apple-flavoured TTML with word-level timing, which is what feeds
Sonora's karaoke rendering. `crates/music/src/lyrics/ttml.rs` already parses that dialect,
so the provider is just the fetch plus the format sniff.

## Why it was added

The stock provider list falls back to whatever the signed-in backend returns — for YouTube
Music that is line-synced text at best. Unison is keyed by YouTube video id, which is
exactly the id Sonora already uses for a YouTube track, so a lookup is a single exact
request rather than a fuzzy title match.

## How a lookup happens

`Unison::search` tries three routes and stops at the first that answers:

| Route | Request | When |
| --- | --- | --- |
| Video id | `GET /lyrics?v=<videoId>` | The track came from YouTube, i.e. `query.id_for("youtube")` is `Some` |
| Metadata | `GET /lyrics?song=&artist=` | No video id, or the id is not in the catalogue |
| Search | `GET /lyrics/search?q=` then `GET /lyrics/:id` | Neither of the above matched |

The search endpoint returns cards *without* lyrics, so each candidate costs a second
request. It is fuzzy enough to return unrelated songs, so `plausible` pre-filters cards
against the query with `lyrics::undecorated` and only the first `CANDIDATES` survivors are
fetched.

A hit found by video id reports the *track's* own title and artist rather than Unison's,
because the id match is already exact and the catalogue's metadata is scraped from YouTube
— titles like `BODY ON ME (Official Visualizer)` would otherwise fail `lyrics::matched`
and get dropped during ranking. This mirrors what `state::lyrics::own` does for the
backend's native lyrics. Hits found by metadata or search keep Unison's metadata so that
ranking can validate them.

## Trust

`lyrics::score` adds `trust` to a hit's score, so it only breaks ties between sheets of the
same depth. Unison's `confidence` field maps onto it:

| Confidence | Trust |
| --- | --- |
| `high` | 240 |
| `medium` | 210 |
| `low` | 190 |

All three sit above the Apple catalogue's 180, so a Unison sheet wins a tie, while the
ordering among Unison's own entries still follows how well the community rates them.

Note that `lyrics::reshape` only conforms hits below `TRUSTED` (25), so Unison sheets are
never re-timed against another provider's guide.

## Formats

`format` is `ttml`, `lrc` or `plain`, but the field can disagree with the payload, so
`sheet` sniffs as well as reads it:

1. TTML if `format` says so *or* the body starts with `<`, via `lyrics::ttml::parse`.
2. Otherwise `lyrics::lrc::parse`, kept if it yields any lines.
3. Otherwise plain text — but never for a body that looked like XML, so a broken TTML
   document is dropped instead of being shown as a wall of markup.

Unison has no instrumental flag, so `instrumental` is always `false`; songs missing from
the catalogue simply return no hits and the other providers fill in.

## Trying it

`lyrics-prober` includes Unison, and prints what each provider returned and which one won:

```sh
cargo prober "https://music.youtube.com/watch?v=Fvfz_mtRwa4"
cargo prober "new body kanye west"
```

The API itself needs no key for reads:

```sh
curl 'https://unison.boidu.dev/lyrics?v=Fvfz_mtRwa4'
```

Writes (submitting and voting on lyrics) are ECDSA-signed and are not implemented here.
