use swarms_rs::agent::SwarmsAgent;
use swarms_rs::llm::provider::openai::OpenAI;
use swarms_rs::structs::tool::Tool;

const SINGLE_SEARCHER_PROMPT: &str = r#"
You search vacation rentals for a large group. Your ONLY goal is to find single
properties that can host the WHOLE group.

Rules:
- Use the search_listings tool; never invent listings.
- Airbnb caps its guest filter at 16, so besides min_guests = group size, also
  search with min_guests = 16 and min_bedrooms around group_size / 3 to catch big
  houses that under-report capacity.
- Run 1 to 3 searches, then reply with a short plain-text summary of the ids found.
"#;

const SPLIT_SEARCHER_PROMPT: &str = r#"
You search vacation rentals for a large group that is willing to split into TWO
places close to each other. Your goal is to find mid-size listings whose
capacities could add up to the group size.

Rules:
- Use the search_listings tool; never invent listings.
- Try different splits, e.g. half/half (min_guests ~ group/2) and 60/40
  (min_guests ~ group*0.4, max_guests ~ group*0.6).
- Also search the complementary small size (e.g. for a 14-guest place, look for
  ~6-guest places) so uneven pairs like 14+6 are found.
- Look at the coordinates: listings close to each other are the most valuable.
- Run 2 to 4 searches, then reply with a short plain-text summary of promising
  nearby combinations (ids only).
"#;

const VETTER_PROMPT: &str = r#"
You vet rental listings for a group trip. For EVERY listing you are given:
- realistic_capacity: adults who can sleep in a real bed or a proper sofa-bed.
  A double bed = 2, single/bunk = 1. Do NOT count floor mattresses, hammocks,
  or "extra guests on request". Never exceed max_guests.
- pool_confirmed: true only if guests can actually use a pool (private, or a
  shared condo/building pool they have access to).
- red_flags: short warnings relevant for a group of friends (shared pool,
  floor mattresses, no parties, noise, no elevator, bunk beds, far from beach...).

Use only the provided data. Call submit_reviews ONCE with all listings, then stop.
"#;

const PRESENTER_PROMPT: &str = r#"
You prepare options for a group vote on where to stay. The group wants to spend
the days together in ONE main house (pool, social space); for "pair" options a
smaller nearby sleeping house is only where a few people sleep.

For EVERY option given, write one sentence (max 30 words) in plain language:
- single: why this one house works for everyone.
- pair: the main house (beach distance, pool, how many sleep there), the walk to
  the sleeping house, and the main trade-off.
Always mention the price per person per night in R$.
Never mention option ids or listing ids. Use only the provided data.
Call submit_pitches ONCE with all options (use the given option_id), then stop.
"#;

fn base(
    client: &OpenAI,
    name: &str,
    prompt: &str,
    max_loops: u32,
) -> swarms_rs::agent::SwarmsAgentBuilder<OpenAI> {
    client
        .agent_builder()
        .agent_name(name)
        .user_name("Trip Planner")
        .system_prompt(prompt)
        .temperature(0.2)
        .max_loops(max_loops)
        .retry_attempts(2)
}

pub fn single_searcher(client: &OpenAI, search: impl Tool + 'static) -> SwarmsAgent<OpenAI> {
    base(client, "Single-Place Searcher", SINGLE_SEARCHER_PROMPT, 3)
        .add_tool(search)
        .build()
}

pub fn split_searcher(client: &OpenAI, search: impl Tool + 'static) -> SwarmsAgent<OpenAI> {
    base(client, "Split Searcher", SPLIT_SEARCHER_PROMPT, 4)
        .add_tool(search)
        .build()
}

pub fn vetter(client: &OpenAI, submit: impl Tool + 'static) -> SwarmsAgent<OpenAI> {
    base(client, "Vetting Agent", VETTER_PROMPT, 1)
        // single shot: the library's task_evaluator tool misparses DeepSeek's args
        .disable_task_complete_tool()
        .add_tool(submit)
        .build()
}

pub fn presenter(client: &OpenAI, submit: impl Tool + 'static) -> SwarmsAgent<OpenAI> {
    base(client, "Presenter Agent", PRESENTER_PROMPT, 1)
        // single shot: the library's task_evaluator tool misparses DeepSeek's args
        .disable_task_complete_tool()
        .add_tool(submit)
        .build()
}
