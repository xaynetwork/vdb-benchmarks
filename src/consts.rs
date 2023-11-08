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

use once_cell::sync::Lazy;

use crate::utils::parse_env;

//WARNING: Has to be kept in sync with values ind docker compose
pub static DOCKER_LIMIT_CPUS: Lazy<f32> = Lazy::new(|| {
    // in number cpus, i.e. compute nodes
    parse_env("DOCKER_LIMIT_CPUS", 4.).unwrap()
});

//WARNING: Has to be kept in sync with values ind docker compose
pub static DOCKER_LIMIT_MEMORY: Lazy<f32> = Lazy::new(|| {
    // in number cpus, i.e. compute nodes
    parse_env("DOCKER_LIMIT_MEM", 8.).unwrap()
});

pub static BENCH_MEASUREMENT_TIME: Lazy<u64> = Lazy::new(|| {
    // FIXME accept 30s
    parse_env("BENCH_MEASUREMENT_TIME", 30).unwrap()
});
