//! Markdown report. OWNER: core agent.

use std::path::{Path, PathBuf};

use crate::domain::{CityPlan, FireworksEvidence, TripRequest};

/// The whole report as markdown: header with the request, one section per
/// city (fireworks evidence with source link, notes, table of stays with
/// price per person per night and links), then the cities without a show.
pub fn render_markdown(
    request: &TripRequest,
    plans: &[CityPlan],
    unconfirmed: &[FireworksEvidence],
) -> String {
    let _ = (request, plans, unconfirmed);
    todo!("core agent")
}

/// Writes `<dir>/<region-slug>_<checkin>.md` and returns its path.
pub fn write_report(dir: &Path, request: &TripRequest, markdown: &str) -> std::io::Result<PathBuf> {
    let _ = (dir, request, markdown);
    todo!("core agent")
}
