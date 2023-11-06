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

use rand::{rngs::StdRng, SeedableRng};
use serde::Deserialize;

#[derive(Deserialize)]
pub(super) struct RngBuilder {
    seed: u64,
}

impl RngBuilder {
    pub(super) fn build(&self) -> StdRng {
        StdRng::seed_from_u64(self.seed)
    }
}
