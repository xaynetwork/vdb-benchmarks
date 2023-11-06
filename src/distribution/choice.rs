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

use anyhow::Error;
use rand::{prelude::Distribution, Rng};
use rand_distr::{Bernoulli, WeightedIndex};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(try_from = "IdxChoiceDistributionSerdeProxy")]
pub(super) struct IdxChoiceDistribution {
    distribution: WeightedIndex<f64>,
    nr_choices: usize,
}

#[derive(Debug, Deserialize)]
#[serde(transparent)]
struct IdxChoiceDistributionSerdeProxy {
    probabilities: Vec<f64>,
}

impl TryFrom<IdxChoiceDistributionSerdeProxy> for IdxChoiceDistribution {
    type Error = Error;
    fn try_from(value: IdxChoiceDistributionSerdeProxy) -> Result<Self, Self::Error> {
        let nr_choices = value.probabilities.len();
        Ok(IdxChoiceDistribution {
            distribution: WeightedIndex::new(value.probabilities)?,
            nr_choices,
        })
    }
}

impl IdxChoiceDistribution {
    pub(super) fn nr_choices(&self) -> usize {
        self.nr_choices
    }
}

impl Distribution<usize> for IdxChoiceDistribution {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> usize {
        self.distribution.sample(rng)
    }
}

#[derive(Debug, Deserialize)]
#[serde(try_from = "BoolDistributionSerdeProxy")]
pub(super) struct BoolDistribution {
    distribution: Bernoulli,
}

impl Distribution<bool> for BoolDistribution {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> bool {
        self.distribution.sample(rng)
    }
}

#[derive(Deserialize)]
#[serde(transparent)]
struct BoolDistributionSerdeProxy {
    probability_true: f64,
}

impl TryFrom<BoolDistributionSerdeProxy> for BoolDistribution {
    type Error = Error;
    fn try_from(source: BoolDistributionSerdeProxy) -> Result<Self, Self::Error> {
        Ok(Self {
            distribution: Bernoulli::new(source.probability_true)?,
        })
    }
}
