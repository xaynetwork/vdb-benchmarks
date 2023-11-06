// Copyright 2023 Xayn AG
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as
// published by the Free Software Foundation, version 3.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

use std::{fs, path::Path};

mod choice;
mod date;
pub mod ids;
pub mod ingestion;
mod label;
mod range;
mod rng;

pub use self::{
    date::Filter as DateFilter,
    label::{Filter as LabelFilter, Label, Labels},
};
use anyhow::Error;
use chrono::{DateTime, Utc};
use rand::{distributions::DistString, rngs::StdRng, Rng};
use rand_distr::{Alphanumeric, Distribution};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct Distributions {
    rng: rng::RngBuilder,
    publication_date: date::Population,
    authors: label::Population,
    tags: label::Population,
}

impl Distributions {
    pub fn load(file: impl AsRef<Path>) -> Result<Self, Error> {
        let bytes = fs::read(file)?;
        let text = String::from_utf8(bytes)?;
        Ok(toml::from_str(&text)?)
    }

    pub fn create_rng(&self) -> StdRng {
        self.rng.build()
    }

    pub fn sample_document_payload<R: Rng + ?Sized>(&self, rng: &mut R) -> DocumentPayload {
        DocumentPayload {
            publication_date: self.publication_date.sample(rng),
            authors: self.authors.sample(rng),
            tags: self.tags.sample(rng),
            link: Alphanumeric.sample_string(rng, 32),
        }
    }

    pub fn sample_query_payload<R: Rng + ?Sized>(&self, rng: &mut R) -> QueryPayload {
        QueryPayload {
            publication_date: self.publication_date.sample(rng),
            authors: self.authors.sample(rng),
            tags: self.tags.sample(rng),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DocumentPayload {
    pub publication_date: DateTime<Utc>,
    pub authors: Labels,
    pub tags: Labels,
    pub link: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct QueryPayload {
    pub publication_date: DateFilter,
    pub authors: LabelFilter,
    pub tags: LabelFilter,
}
