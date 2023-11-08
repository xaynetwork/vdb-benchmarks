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

use std::time::Duration;

use anyhow::Error;
use criterion::Criterion;
use vdb_benchmarks::{
    benchmarks::query_throughput,
    databases::elasticsearch::Elasticsearch,
    resources::{ResolvedPaths, ResourceWriter},
    utils::parse_env,
};

fn main() -> Result<(), Error> {
    let time_in_seconds = parse_env("BENCH_MEASUREMENT_TIME", 10)?;
    let mut c = Criterion::default()
        .configure_from_args()
        .measurement_time(Duration::from_secs(time_in_seconds))
        .sample_size(10);
    let writer = ResourceWriter::new("./reports/additional_data", ["elasticsearch"])?;
    let paths = ResolvedPaths::resolve("./resources/gist-960-euclidean.hdf5");
    let database = Elasticsearch::new()?;
    query_throughput::benchmark(&writer, &paths, database, &mut c)?;
    writer.write_close_msg()?;
    c.final_summary();
    Ok(())
}
