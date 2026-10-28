//! Shared state the agents write into through tools. OWNER: core agent.

use std::sync::{Arc, Mutex};

use crate::domain::{CandidateCity, CityPlan, FireworksEvidence, Listing, TripRequest};

#[derive(Debug, Default)]
struct PlanState {
    _todo: (),
}

/// Cheap to clone; every clone sees the same state.
#[derive(Debug, Clone, Default)]
pub struct SharedState(Arc<Mutex<PlanState>>);

impl SharedState {
    /// Cities proposed by the region planner, in its order.
    pub fn candidates(&self) -> Vec<CandidateCity> {
        todo!("core agent")
    }

    /// Up to `max` cities with a confirmed fireworks show, in planner order.
    pub fn confirmed(&self, max: usize) -> Vec<(CandidateCity, FireworksEvidence)> {
        let _ = max;
        todo!("core agent")
    }

    /// Researched cities without a confirmed show, for the report's "not found" list.
    pub fn unconfirmed(&self) -> Vec<FireworksEvidence> {
        todo!("core agent")
    }

    pub fn set_listings(&self, city: &str, listings: Vec<Listing>) {
        let _ = (city, listings);
        todo!("core agent")
    }

    /// Text handed to the curator: per city, the candidate listings (id, price
    /// per person per night, capacity, rooms, rating, pool, short description).
    pub fn curation_brief(&self, request: &TripRequest, per_city: usize) -> String {
        let _ = (request, per_city);
        todo!("core agent")
    }

    /// Final per-city results for the report, in planner order.
    pub fn plans(&self) -> Vec<CityPlan> {
        todo!("core agent")
    }
}
