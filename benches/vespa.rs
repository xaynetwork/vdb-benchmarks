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
    databases::vespa::Vespa,
    resources::{ResolvedPaths, ResourceWriter},
};

fn main() -> Result<(), Error> {
    let mut c = Criterion::default()
        .configure_from_args()
        .measurement_time(Duration::from_secs(10))
        .sample_size(10);
    let writer = ResourceWriter::new("./reports/additional_data", ["vespa"])?;
    let paths = ResolvedPaths::resolve("./resources/gist-960-euclidean.hdf5");
    let database = Vespa::new(0)?;
    query_throughput::benchmark(&writer, &paths, database, &mut c)?;
    c.final_summary();
    writer.write_close_msg()?;
    Ok(())
}
