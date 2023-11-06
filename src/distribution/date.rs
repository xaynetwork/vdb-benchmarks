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

use std::mem;

use anyhow::{anyhow, Error};
use chrono::{DateTime, Utc};
use rand_distr::Distribution;
use serde::{Deserialize, Serialize};

use super::{
    choice::BoolDistribution,
    range::{RangeDistribution, RangeDistributionBuilder},
};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Filter {
    pub lower_bound: Option<DateTime<Utc>>,
    pub upper_bound: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
#[serde(try_from = "DatePopulationSerdeProxy")]
pub(super) struct Population {
    sample_distribution: DateRangeDistribution,
    filters: FilterDistributions,
}

#[derive(Debug)]
struct FilterDistributions {
    has_upper_bound: BoolDistribution,
    has_lower_bound: BoolDistribution,
    upper_bound_sample_distribution: DateRangeDistribution,
    lower_bound_sample_distribution: DateRangeDistribution,
}

impl Distribution<DateTime<Utc>> for Population {
    fn sample<R: rand::Rng + ?Sized>(&self, rng: &mut R) -> DateTime<Utc> {
        self.sample_distribution.sample(rng)
    }
}

impl Distribution<Filter> for Population {
    fn sample<R: rand::Rng + ?Sized>(&self, rng: &mut R) -> Filter {
        let mut lower_bound = self
            .filters
            .has_lower_bound
            .sample(rng)
            .then(|| self.filters.lower_bound_sample_distribution.sample(rng));
        let mut upper_bound = self
            .filters
            .has_upper_bound
            .sample(rng)
            .then(|| self.filters.upper_bound_sample_distribution.sample(rng));

        if let (Some(lower_bound), Some(upper_bound)) = (&mut lower_bound, &mut upper_bound) {
            if lower_bound > upper_bound {
                mem::swap(lower_bound, upper_bound);
            }
        }

        Filter {
            lower_bound,
            upper_bound,
        }
    }
}

#[derive(Debug, Deserialize)]
struct DatePopulationSerdeProxy {
    min: DateTime<Utc>,
    max: DateTime<Utc>,
    sample_distribution: RangeDistributionBuilder,
    filters: DateFiltersSerdeProxy,
}

#[derive(Debug, Deserialize)]
struct DateFiltersSerdeProxy {
    has_upper_bound: BoolDistribution,
    has_lower_bound: BoolDistribution,
    upper_bound_sample_distribution: RangeDistributionBuilder,
    lower_bound_sample_distribution: RangeDistributionBuilder,
}

impl TryFrom<DatePopulationSerdeProxy> for Population {
    type Error = Error;

    fn try_from(source: DatePopulationSerdeProxy) -> Result<Self, Self::Error> {
        let to_u64 = |bound: DateTime<Utc>| -> Result<u64, Error> {
            bound
                .timestamp()
                .try_into()
                .map_err(|_| anyhow!("Only positive epoch times are supported."))
        };

        let min = to_u64(source.min)?;
        let max = to_u64(source.max)?;

        Ok(Population {
            sample_distribution: source.sample_distribution.build(min, max)?.into(),

            filters: FilterDistributions {
                has_lower_bound: source.filters.has_lower_bound,
                has_upper_bound: source.filters.has_upper_bound,
                upper_bound_sample_distribution: source
                    .filters
                    .upper_bound_sample_distribution
                    .build(min, max)?
                    .into(),
                lower_bound_sample_distribution: source
                    .filters
                    .lower_bound_sample_distribution
                    .build(min, max)?
                    .into(),
            },
        })
    }
}

#[derive(Debug)]
pub(super) struct DateRangeDistribution(RangeDistribution);

impl From<RangeDistribution> for DateRangeDistribution {
    fn from(value: RangeDistribution) -> Self {
        Self(value)
    }
}

impl Distribution<DateTime<Utc>> for DateRangeDistribution {
    fn sample<R: rand::Rng + ?Sized>(&self, rng: &mut R) -> DateTime<Utc> {
        let timestamp = self.0.sample(rng);
        // assumption: RangeDistribution is [0;...), if the assumption brakes we clamp
        DateTime::from_timestamp(timestamp.max(0) as _, 0).unwrap()
    }
}
