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

use anyhow::{bail, Error};
use rand_distr::{Distribution, Normal, Uniform};
use serde::{de, Deserialize, Deserializer};

#[derive(Debug)]
pub(super) struct RangeDistribution {
    inner: InnerRangeDistribution,
}

impl Distribution<u64> for RangeDistribution {
    fn sample<R: rand::Rng + ?Sized>(&self, rng: &mut R) -> u64 {
        match &self.inner {
            InnerRangeDistribution::Uniform(uniform) => uniform.sample(rng),
            InnerRangeDistribution::Normal {
                distribution,
                mean,
                range_len,
                range_min,
            } => {
                let mut point = distribution.sample(rng).round();
                // mathematically not perfect, good enough for this use-case
                while point < 0. {
                    // point = mean - (abs_dist(point, 0))
                    point += mean;
                }
                while point > *range_len {
                    // point = mean + (abs_dist(point, range_len))
                    point = mean + (point - range_len)
                }
                range_min + point as u64
            }
        }
    }
}

#[derive(Debug)]
enum InnerRangeDistribution {
    Uniform(Uniform<u64>),
    Normal {
        distribution: Normal<f64>,
        mean: f64,
        range_len: f64,
        range_min: u64,
    },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum RangeDistributionBuilder {
    Uniform,
    Normal { mean: Percentage, std: Percentage },
}

impl RangeDistributionBuilder {
    pub fn build(&self, range_min: u64, range_max: u64) -> Result<RangeDistribution, Error> {
        if range_min >= range_max {
            bail!("range_min must be < range_max");
        }

        let inner = match self {
            RangeDistributionBuilder::Uniform => {
                let distribution = Uniform::new_inclusive(range_min, range_max);
                InnerRangeDistribution::Uniform(distribution)
            }
            RangeDistributionBuilder::Normal { mean, std } => {
                let range_len = (range_max - range_min) as f64;
                let mean = range_len * mean.0;
                let std2 = (range_len * std.0).powi(2);
                let distribution = Normal::new(mean, std2)?;
                InnerRangeDistribution::Normal {
                    distribution,
                    mean,
                    range_len,
                    range_min,
                }
            }
        };
        Ok(RangeDistribution { inner })
    }
}

#[derive(Debug)]
pub(super) struct Percentage(f64);

impl<'de> Deserialize<'de> for Percentage {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        if value.ends_with('%') {
            if let Ok(p) = value[..value.len() - 1].parse::<f64>() {
                if (0. ..=100.).contains(&p) {
                    return Ok(Self(p / 100.));
                }
            }
        }

        Err(de::Error::invalid_value(
            de::Unexpected::Str(&value),
            &"percentage value like \"10%\"",
        ))
    }
}
