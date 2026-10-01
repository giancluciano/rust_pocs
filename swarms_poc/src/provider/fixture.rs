use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::{ListingProvider, ProviderError, ProviderFuture, SearchQuery};
use crate::domain::{Destination, Listing};

#[derive(Debug, Deserialize)]
struct FixtureFile {
    destination: Destination,
    listings: Vec<Listing>,
}

/// Reads `<dir>/<city-slug>.json`, e.g. `fixtures/florianopolis.json`.
pub struct FixtureProvider {
    destination: Destination,
    listings: Vec<Listing>,
}

impl FixtureProvider {
    pub fn load(dir: &Path, city: &str) -> Result<Self, ProviderError> {
        let path = fixture_path(dir, city);
        if !path.exists() {
            return Err(ProviderError::FixtureNotFound {
                city: city.to_string(),
                path: path.display().to_string(),
            });
        }
        let file: FixtureFile = serde_json::from_str(&std::fs::read_to_string(&path)?)?;
        Ok(Self {
            destination: file.destination,
            listings: file.listings,
        })
    }
}

impl ListingProvider for FixtureProvider {
    fn destination(&self) -> &Destination {
        &self.destination
    }

    fn search<'a>(&'a self, query: &'a SearchQuery) -> ProviderFuture<'a, Vec<Listing>> {
        let found = self
            .listings
            .iter()
            .filter(|l| query.matches(l))
            .cloned()
            .collect();
        Box::pin(async move { Ok(found) })
    }
}

fn fixture_path(dir: &Path, city: &str) -> PathBuf {
    dir.join(format!("{}.json", slugify(city)))
}

/// "Florianópolis" -> "florianopolis", "Rio de Janeiro" -> "rio-de-janeiro".
pub fn slugify(city: &str) -> String {
    let folded: String = city
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ã' | 'ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            'ñ' => 'n',
            c if c.is_ascii_alphanumeric() => c,
            _ => '-',
        })
        .collect();
    folded
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Stay;
    use crate::domain::test_support::date;

    fn fixtures_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures")
    }

    fn query(min_guests: u32) -> SearchQuery {
        SearchQuery {
            stay: Stay::new(date("2027-01-10"), date("2027-01-17")).unwrap(),
            min_guests,
            max_guests: None,
            min_bedrooms: None,
            max_price_per_night: None,
        }
    }

    #[test]
    fn slugify_folds_accents_and_spaces() {
        assert_eq!(slugify("Florianópolis"), "florianopolis");
        assert_eq!(slugify("  Rio de Janeiro "), "rio-de-janeiro");
        assert_eq!(slugify("São Paulo"), "sao-paulo");
    }

    #[test]
    fn missing_fixture_is_a_clear_error() {
        let err = FixtureProvider::load(&fixtures_dir(), "Atlantis")
            .err()
            .unwrap();
        assert!(err.to_string().contains("atlantis.json"));
    }

    #[tokio::test]
    async fn loads_florianopolis_and_filters() {
        let provider = FixtureProvider::load(&fixtures_dir(), "Florianópolis").unwrap();
        assert_eq!(provider.destination().city, "Florianópolis");
        assert!(!provider.destination().beaches.is_empty());

        let big = provider.search(&query(20)).await.unwrap();
        assert!(!big.is_empty());
        assert!(big.iter().all(|l| l.max_guests >= 20));
    }

    #[tokio::test]
    async fn search_excludes_booked_listings() {
        let provider = FixtureProvider::load(&fixtures_dir(), "florianopolis").unwrap();
        let all = provider.search(&query(1)).await.unwrap();
        assert!(
            all.iter().all(|l| l.id != "fx-014"),
            "fx-014 is booked 12-15 Jan 2027"
        );
    }
}
