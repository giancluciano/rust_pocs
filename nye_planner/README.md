# New Year's Eve travel planner (swarms-rs PoC)

Give it a region and a group size. It finds cities in that region that
announced a New Year's Eve fireworks show, then Airbnb stays that fit the
whole group, and writes a markdown report.

## Run

```bash
# .env
DEEPSEEK_BASE_URL=https://api.deepseek.com
DEEPSEEK_API_KEY=...
BRAVE_API_KEY=...            # recommended, see "Web search" below
# DEEPSEEK_MODEL=deepseek-chat   (optional)
# OSM_CONTACT=you@example.com    (optional, identifies you to OpenStreetMap)

cargo run --release -- --region "the coastline of Santa Catarina" --people 12 \
  --checkin 2026-12-30 --checkout 2027-01-02
```

The stay must include the night of December 31. Options: `--max-candidates 15`
(cities researched), `--max-cities 10` (confirmed cities kept), `--per-city 10`
(stays per city), `--output-dir output`, `--cache-dir cache`. Log level:
`RUST_LOG` (default `nye_planner=info,warn`).

A full run takes several minutes: Airbnb is scraped at one request per second.
Results are cached (searches and pages 24 h, Airbnb 12 h, OpenStreetMap 30
days), so a second run with the same input is fast.

## Pipeline (`src/main.rs`)

```
Region Planner                 one agent           tool: submit_candidates
      │  candidate cities
      ▼
Fireworks Researchers          ConcurrentWorkflow  tools: web_search, read_page,
      │  one agent per city, in parallel           submit_fireworks
      │  confirmed only with a source for this New Year's Eve
      ▼
Airbnb search                  plain code          src/airbnb/
      │  entire homes for the group, cheapest first, detail pages for capacity
      ▼
Stay Curator → Travel Writer   SequentialWorkflow  tools: submit_picks,
      │                                            get_city_facts, submit_city_notes
      ▼
output/<region>_<checkin>.md
```

Agents hand results back through tools that write into shared state
(`src/state.rs`); the code never parses an agent's free-text reply. Each
agent's last submit tool appends a `<<DONE>>` marker to its result, which is
registered as a stop word, so the agent stops right after reporting. If the
curator picks nothing for a city, the report falls back to the cheapest stays
that fit the group.

## Web search

- **Brave Search API** when `BRAVE_API_KEY` is set (free plan, 1 request per
  second). Get a key at https://brave.com/search/api/.
- **DuckDuckGo** HTML endpoint otherwise. No key, but it blocks an IP after a
  handful of searches, which is fewer than one researcher per city needs.

`read_page` only fetches public `http`/`https` URLs: localhost, private and
link-local addresses are refused, including after redirects, because the URLs
come from the model.

## Airbnb

Ported from `../swarms_poc`: OpenStreetMap (Nominatim) gives each city's
bounding box, then Airbnb's search and listing pages are read from the JSON
embedded in the HTML. One shared client for all cities: one request per
second plus jitter, a warm-up visit, a disk cache, a request budget, and a
circuit breaker that stops on a block or captcha page.
