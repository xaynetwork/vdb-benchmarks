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

use std::fmt::{Debug, Display};

use anyhow::{bail, Error};
use derive_more::Deref;
use rand::{prelude::Distribution, Rng};
use rand_distr::WeightedAliasIndex;
use serde::{Deserialize, Serialize};

use super::{choice::IdxChoiceDistribution, ids::index_to_fake_uuid};

#[derive(PartialEq, Serialize, Deserialize)]
#[repr(transparent)]
pub struct Label(pub u64);

impl Display for Label {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", index_to_fake_uuid(self.0))
    }
}

impl Debug for Label {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.to_string())
    }
}

#[derive(Debug, Default, Deref, Serialize, Deserialize)]
#[repr(transparent)]
pub struct Labels(pub Vec<Label>);

impl Labels {
    pub fn to_uuid_string_vec(&self) -> Vec<String> {
        self.iter().map(Label::to_string).collect()
    }
}

#[derive(Debug, Deserialize)]
#[serde(try_from = "LabelPopulationSerdeProxy")]
pub(super) struct Population {
    property_count_distribution: IdxChoiceDistribution,
    filters: FilterGenerator,
    population: usize,
    distribution: WeightedAliasIndex<f64>,
}

impl Population {
    fn sample_n_unique(&self, n: usize, rng: &mut (impl Rng + ?Sized)) -> Labels {
        // sanity checks to avoid infinite or absurdly long running programs
        assert!(
            n <= self.population / 2,
            "unique label count larger then half the population"
        );
        assert!(
            n <= 512,
            "label count sampling is written for small n (<=512), got: {n}"
        );

        let mut labels = Vec::with_capacity(n);
        while labels.len() < n {
            // assumption: count isn't to big
            let label: Label = self.sample(rng);
            if labels.contains(&label) {
                continue;
            }
            labels.push(label);
        }
        Labels(labels)
    }
}

impl Distribution<Label> for Population {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> Label {
        Label(self.distribution.sample(rng).try_into().unwrap())
    }
}

impl Distribution<Labels> for Population {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> Labels {
        let n = self.property_count_distribution.sample(rng);
        self.sample_n_unique(n, rng)
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Filter {
    pub include: Labels,
    pub exclude: Labels,
}

impl Distribution<Filter> for Population {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> Filter {
        let nr_include = self.filters.include_count_distribution.sample(rng);
        let nr_exclude = self.filters.exclude_count_distribution.sample(rng);
        let mut labels: Labels = self.sample_n_unique(nr_include + nr_exclude, rng);
        let exclude = Labels(labels.0.split_off(nr_include));
        Filter {
            include: labels,
            exclude,
        }
    }
}

#[derive(Debug, Deserialize)]
struct LabelPopulationSerdeProxy {
    population: usize,
    zipfs_law_pmf: ZipfsLawPmfSettings,
    property_count_distribution: IdxChoiceDistribution,
    filters: FilterGenerator,
}

impl TryFrom<LabelPopulationSerdeProxy> for Population {
    type Error = Error;
    fn try_from(value: LabelPopulationSerdeProxy) -> Result<Self, Self::Error> {
        if value.population > 1_000_000 {
            bail!("Implementation only works reasonable for limited populations (max: 1_000_000)");
        }

        // generate weights based on Zipf's Law
        let mut weights = (1..=value.population)
            // \frac{1}{k^s}
            .map(|k| (k as f64).powf(value.zipfs_law_pmf.s).recip())
            .collect::<Vec<_>>();
        // H_{N,s} = \sum_{k=1}^{N}\frac{1}{k^s} = \sum_{k=1}^{N}weight_k
        let hns: f64 = weights.iter().copied().sum();
        for weight in &mut weights {
            *weight /= hns;
        }

        let distribution = WeightedAliasIndex::new(weights).unwrap();

        let max_allowed_samples = value.population / 2;
        let max_samples = value.property_count_distribution.nr_choices();
        if max_samples > max_allowed_samples {
            bail!("Cannot sample {max_samples} labels (max allowed: {max_allowed_samples})");
        }
        let total_max_filters = value.filters.exclude_count_distribution.nr_choices()
            + value.filters.include_count_distribution.nr_choices();
        if total_max_filters > value.population / 2 {
            bail!("Cannot sample {total_max_filters} filter labels (max allowed: {max_allowed_samples})");
        }

        Ok(Population {
            distribution,
            property_count_distribution: value.property_count_distribution,
            filters: value.filters,
            population: value.population,
        })
    }
}

#[derive(Debug, Deserialize)]
struct ZipfsLawPmfSettings {
    s: f64,
}

#[derive(Debug, Deserialize)]
struct FilterGenerator {
    include_count_distribution: IdxChoiceDistribution,
    exclude_count_distribution: IdxChoiceDistribution,
}
