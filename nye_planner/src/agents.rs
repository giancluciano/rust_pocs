//! Agent definitions: system prompts + tools.
//!
//! Every agent ends by calling a `submit_*` tool. Its final result carries
//! `DONE_MARKER`, which is registered as a stop word, so the agent loop stops
//! right after the last submit instead of running until `max_loops`. The
//! built-in `task_evaluator` tool is switched off: one fewer tool to confuse
//! the model, and no early "complete" before the submit.

use chrono::Datelike;
use swarms_rs::agent::{SwarmsAgent, SwarmsAgentBuilder};
use swarms_rs::llm::provider::openai::OpenAI;

use crate::domain::{CandidateCity, TripRequest};
use crate::state::SharedState;
use crate::tools::{
    DONE_MARKER, GetCityFactsTool, ReadPageTool, SubmitCandidatesTool, SubmitCityNotesTool,
    SubmitFireworksTool, SubmitPicksTool, WebSearchTool,
};
use crate::websearch::WebSearch;

const TEMPERATURE: f64 = 0.2;
const RETRY_ATTEMPTS: u32 = 2;
const PLANNER_LOOPS: u32 = 3;
/// search x3, read x2, submit, plus a retry after a rejected submit.
const RESEARCHER_LOOPS: u32 = 8;
/// One loop can hold several tool calls, but a model may go city by city.
const CURATOR_LOOPS: u32 = 15;
const WRITER_LOOPS: u32 = 20;

const PLANNER_PROMPT: &str = r#"
You are a travel planner. Given a region, propose real cities in it that are
likely to host New Year's Eve celebrations with fireworks.

Rules:
- Propose at most {max_cities} cities. Prefer well-known reveillon destinations,
  best known first.
- Use only cities that really exist in the region. Never invent places.
- Give each city's name only (no state in the name), its state or province, its
  country and one short reason.
- Call submit_candidates exactly once with the full list, then stop.
"#;

const RESEARCHER_PROMPT: &str = r#"
You check whether {city} announced a fireworks show for New Year's Eve {eve}
(the night of December 31, {year}, into {next_year}).

Rules:
- Search in the LOCAL language of the city. For Brazil, for example:
  "réveillon {year} queima de fogos {name}" and "virada {year} {name} fogos".
- Use 1 to 3 web_search calls, then read the most promising official or news
  page with read_page.
- Set confirmed=true ONLY if a source explicitly announces fireworks for THIS
  New Year's Eve ({year}). Past years do not count, and neither does "it
  happens every year" without a {year} announcement. Mention such findings in
  the summary and set confirmed=false.
- Never guess or invent. No source means confirmed=false.
- confirmed=true needs source_url, the page that announces the show. The
  summary says where and when, and who announced it.
- Call submit_fireworks exactly once, then stop.
"#;

const CURATOR_PROMPT: &str = r#"
You choose Airbnb stays for a group on a New Year's Eve trip. The task is a brief
with the group size, the dates and, per city, candidate listings.

Rules:
- For each city that has listings, pick up to {per_city} stays that work for the
  WHOLE group, best first. Skip cities that have no listings.
- Weigh capacity (it must hold the group), price per person per night, rating
  and number of reviews, pool and the red flags in the description (shared
  pool, party or event bans, far from the beach, strict rules, few reviews).
- Use only listing ids from the brief, copied exactly. Never invent listings.
- Each reason is one short sentence based on the data.
- Call submit_picks once per city. When its result says all cities have picks,
  stop.
"#;

const WRITER_PROMPT: &str = r#"
You are a travel writer. The input is the work of the stay curator: the cities of
a New Year's Eve trip and the stays picked for each.

Rules:
- For every city in the input: call get_city_facts, then call submit_city_notes.
- Notes are 2 to 3 sentences in English: the fireworks show (where and when,
  exactly as the source says) and what the picked stays are like (price per
  person per night, capacity, pool, rating).
- Use only what get_city_facts returns. Never invent facts. If the show is not
  confirmed or a detail is missing, leave it out.
- Keep each city under 600 characters.
- When submit_city_notes says all cities have notes, stop.
"#;

fn base(llm: &OpenAI, name: &str, prompt: String, max_loops: u32) -> SwarmsAgentBuilder<OpenAI> {
    llm.agent_builder()
        .agent_name(name)
        .user_name("NYE Planner")
        .system_prompt(prompt)
        .temperature(TEMPERATURE)
        .max_loops(max_loops)
        .retry_attempts(RETRY_ATTEMPTS)
        .disable_task_complete_tool()
        .add_stop_word(DONE_MARKER)
}

/// Turns the free-text region into up to `max_cities` candidate cities.
/// Reports them with the `submit_candidates` tool.
pub fn region_planner(llm: &OpenAI, state: SharedState, max_cities: usize) -> SwarmsAgent<OpenAI> {
    let prompt = PLANNER_PROMPT.replace("{max_cities}", &max_cities.to_string());
    base(llm, "Region Planner", prompt, PLANNER_LOOPS)
        .add_tool(SubmitCandidatesTool::new(state, max_cities))
        .build()
}

/// Checks the web for a New Year's Eve fireworks show in one city.
/// Tools: `web_search`, `read_page`, `submit_fireworks`.
pub fn fireworks_researcher(
    llm: &OpenAI,
    city: &CandidateCity,
    request: &TripRequest,
    web: WebSearch,
    state: SharedState,
) -> SwarmsAgent<OpenAI> {
    let name = format!("Fireworks Researcher: {}", city.name);
    base(llm, &name, researcher_prompt(city, request), RESEARCHER_LOOPS)
        .add_tool(WebSearchTool::new(web.clone()))
        .add_tool(ReadPageTool::new(web))
        .add_tool(SubmitFireworksTool::new(state, city.clone()))
        .build()
}

fn researcher_prompt(city: &CandidateCity, request: &TripRequest) -> String {
    let eve = request.stay.new_years_eve().unwrap_or_else(|| request.stay.checkin());
    RESEARCHER_PROMPT
        .replace("{city}", &city.full_name())
        .replace("{name}", &city.name)
        .replace("{eve}", &eve.to_string())
        .replace("{year}", &eve.year().to_string())
        .replace("{next_year}", &(eve.year() + 1).to_string())
}

/// Picks up to `per_city` listings per city with a reason. Tool: `submit_picks`.
pub fn stay_curator(llm: &OpenAI, state: SharedState, per_city: usize) -> SwarmsAgent<OpenAI> {
    let prompt = CURATOR_PROMPT.replace("{per_city}", &per_city.to_string());
    base(llm, "Stay Curator", prompt, CURATOR_LOOPS)
        .add_tool(SubmitPicksTool::new(state, per_city))
        .build()
}

/// Writes a short paragraph per city from the curator's summary and the
/// fireworks evidence. Tools: `get_city_facts`, `submit_city_notes`.
pub fn travel_writer(llm: &OpenAI, state: SharedState) -> SwarmsAgent<OpenAI> {
    base(llm, "Travel Writer", WRITER_PROMPT.to_string(), WRITER_LOOPS)
        .add_tool(GetCityFactsTool::new(state.clone()))
        .add_tool(SubmitCityNotesTool::new(state))
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Stay;
    use chrono::NaiveDate;

    fn request() -> TripRequest {
        let d = |s: &str| NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap();
        TripRequest {
            region: "Santa Catarina".into(),
            people: 8,
            stay: Stay::new(d("2026-12-30"), d("2027-01-02")).unwrap(),
        }
    }

    fn city() -> CandidateCity {
        CandidateCity {
            name: "Bombinhas".into(),
            state: "Santa Catarina".into(),
            country: "Brazil".into(),
            why: String::new(),
        }
    }

    #[test]
    fn researcher_prompt_names_the_city_and_the_year() {
        let prompt = researcher_prompt(&city(), &request());
        assert!(prompt.contains("Bombinhas, Santa Catarina, Brazil"));
        assert!(prompt.contains("2026-12-31"));
        assert!(prompt.contains("réveillon 2026 queima de fogos Bombinhas"));
        assert!(prompt.contains("into 2027"));
        assert!(!prompt.contains('{'), "unfilled placeholder: {prompt}");
    }

    #[test]
    fn prompts_have_no_leftover_placeholders() {
        let planner = PLANNER_PROMPT.replace("{max_cities}", "5");
        let curator = CURATOR_PROMPT.replace("{per_city}", "3");
        assert!(!planner.contains('{') && !curator.contains('{'));
        assert!(!WRITER_PROMPT.contains('{'));
    }
}
