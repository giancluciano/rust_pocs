# Group rental researcher (swarms-rs PoC)

Finds vacation rentals for a big group (~20 people): **one place that fits
everyone**, or **two places close to each other**. It ranks the options and
writes them to CSV so the group can vote.

## Run

```bash
# .env
DEEPSEEK_BASE_URL=https://api.deepseek.com
DEEPSEEK_API_KEY=...
# DEEPSEEK_MODEL=deepseek-chat   (optional)

cargo run -- --city "Florianópolis" --people 20 --checkin 2027-01-10 --checkout 2027-01-17
```

Add `--source airbnb` to scrape airbnb.com instead of the offline fixture:

```bash
cargo run -- --source airbnb --city "Florianópolis" --people 20 \
  --checkin 2027-01-10 --checkout 2027-01-17 --currency BRL
```

Options: `--max-pair-distance-m 500`, `--top 10`, `--fixtures-dir fixtures`,
`--output-dir output`. Scraper options: `--max-pages 2` (per search),
`--max-listing-details 60` (per run), `--request-interval-ms 1000` (1000 is the
minimum), `--cache-dir cache`. Set `OSM_CONTACT` (e.g. an email) to identify
yourself to OpenStreetMap as its usage policy asks. Set log level with `RUST_LOG` (default `swarms_poc=info,warn`).

## Pipeline

```
[Single-Place Searcher ‖ Split Searcher]   agents, tool: search_listings
        │  (listings + distances to center / nearest beach + pool flag, computed in code)
        ▼
Vetting Agent                               tool: submit_reviews
        │  realistic capacity (no floor mattresses), pool confirmed, red flags
        ▼
Build singles + nearby pairs, score         deterministic code (src/options.rs, src/scoring.rs)
        ▼
Presenter Agent                             tool: submit_pitches — one-line pitch per option
        ▼
output/<city>_<checkin>_options.csv   (one row per voting option, empty `votes` column)
output/<city>_<checkin>_listings.csv  (every listing found, for traceability)
```

Agents pass data to each other through tools that write to shared state. The
code never parses the model's free text. Distances, pairing and scoring are
computed in code so the ranking is reproducible. The LLM handles judgment
calls only: search strategy, reading descriptions, and writing pitches. If an
agent fails, the pipeline falls back: a broad search, the host-declared
capacity, or no pitch.

Score (0–100) weights: beach 30%, center 20%, pool 20%, price per person 20%,
rating 10% (`Weights::default()` in `src/scoring.rs`). For pairs, distances are
the worse of the two places.

## Data sources

- **fixture** (default): `fixtures/<city-slug>.json`, offline. Only
  `florianopolis.json` exists.
- **airbnb** (`src/provider/airbnb/`):
  1. OpenStreetMap: Nominatim gives the city center and bounding box, and
     Overpass gives the beaches (`natural=beach`).
  2. Search pages (`/s/<city>/homes`, limited to the city's bounding box and
     to entire homes) give the id, coordinates, bedrooms, beds and the total
     price for the dates.
  3. Listing pages (`/rooms/<id>`) give the guest capacity, amenities (pool),
     description (for the vetting agent) and rating. They are fetched once per
     listing per run, up to `--max-listing-details`. Listings beyond that keep
     only search data, with capacity = the guest filter as a lower bound.

  Both read the JSON Airbnb embeds in the page (`data-deferred-state-0`), so no
  browser is needed.

### How the scraper avoids getting blocked (`src/http.rs`)

- **1 request/second, one at a time**: a single shared limiter for all agents,
  plus 0–400 ms of random jitter.
- **Consistent browser session**: one desktop Chrome User-Agent and normal
  `Accept`/`Accept-Language` headers, a cookie jar, and a home-page visit to
  open the session. Listing pages send the search page as `Referer`.
- **Backoff**: 429 and 5xx responses pause *all* requests (honoring
  `Retry-After`, otherwise 5 s, 10 s, ...).
- **Circuit breaker**: a 403, repeated 429s or a captcha page stop all further
  requests for the rest of the run instead of retrying into a ban.
- **Low volume**: a per-run request budget, page caps, and a disk cache (12 h
  for Airbnb, 30 days for OpenStreetMap), so reruns and overlapping agent
  searches cost almost nothing.

It does **not** rotate IPs or identities, use proxies, or solve captchas. If
Airbnb pushes back, the run stops and tells you. Airbnb's Terms of Service
forbid scraping, and its robots.txt disallows `/s/*/*` (the search path; the
single-segment `/s/<city>` returns 404). Keep it to small, personal runs.

## Known limitations

- Distances are straight-line (haversine), not walking or driving distances.
- Airbnb shows approximate locations (shifted by up to a few hundred meters)
  until you book, so pair distances are estimates.
- The scraper depends on Airbnb's page structure. If it changes, you'll get a
  `could not understand the page` error; update `provider/airbnb/parse.rs`.
- swarms-rs 0.2.1 panics when the model sends malformed tool-call JSON. Each
  agent runs in its own task and is retried once (`pipeline::run_agent`).
