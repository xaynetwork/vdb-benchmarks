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

use serde::{Deserialize, Serialize};

/// https://en.wikipedia.org/wiki/Algorithms_for_calculating_variance#Welford's_online_algorithm
#[derive(Default, Serialize, Deserialize)]
pub struct WelfordOnlineAlgorithm {
    count: usize,
    mean: f64,
    m2: f64,
}

impl WelfordOnlineAlgorithm {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn update(&mut self, new_value: f64) {
        self.count += 1;
        let diff1 = new_value - self.mean;
        self.mean += diff1 / self.count as f64;
        let diff2 = new_value - self.mean;
        self.m2 += diff1 * diff2;
    }

    pub fn mean(&self) -> f64 {
        self.mean
    }

    pub fn variance(&self) -> f64 {
        self.m2 / self.count as f64
    }

    pub fn sample_variance(&self) -> f64 {
        self.m2 / (self.count as f64 - 1.)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn welford_works_for_simple_cases() {
        let mut wf = WelfordOnlineAlgorithm::default();
        for value in [1.2, 3.2, 12.3] {
            wf.update(value);
        }
        assert_eq!(wf.mean(), 5.566666666666667);
        assert_eq!(wf.variance().sqrt(), 4.830688931773144);
        assert_eq!(wf.sample_variance().sqrt(), 5.916361494477272);
    }
}
