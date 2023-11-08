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

use std::path::PathBuf;

use anyhow::bail;
use clap::Parser;
use tokio::runtime::Runtime;
use vdb_benchmarks::{
    databases::{elasticsearch::Elasticsearch, qdrant::Qdrant, vespa::Vespa},
    distribution::ingestion::ingest_database,
    resources::{ResolvedPaths, ResourceWriter},
};

#[derive(Parser, Debug)]
#[command(version)]
struct Cli {
    #[arg(long)]
    vectors: PathBuf,

    #[arg(short, long)]
    provider: String,
}

fn main() -> Result<(), anyhow::Error> {
    let args = Cli::parse();
    let writer = ResourceWriter::new("./reports/additional_data", [args.provider.as_str()])?;
    let paths = ResolvedPaths::resolve(args.vectors);

    let rt = Runtime::new()?;
    match args.provider.as_str() {
        "qdrant" => {
            let database = Qdrant::new(1)?;
            rt.block_on(async { ingest_database(&writer, &paths, &database).await })?;
        }
        "vespa" => {
            let database = Vespa::new(0)?;
            rt.block_on(async { ingest_database(&writer, &paths, &database).await })?;
        }
        "elasticsearch" => {
            let database = Elasticsearch::new()?;
            rt.block_on(async { ingest_database(&writer, &paths, &database).await })?;
        }
        unknown => bail!("unknown provider: {unknown}"),
    }
    writer.write_close_msg()?;
    Ok(())
}
