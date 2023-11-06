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

use std::path::{Path, PathBuf};

use anyhow::Error;
use clap::Parser;
use serde::Serialize;
use vdb_benchmarks::{
    distribution::Distributions,
    resources::{dump_bincode, ResolvedPaths},
};

#[derive(Parser, Debug)]
#[command(version)]
struct Cli {
    #[arg(long)]
    vectors: PathBuf,
}

fn main() -> Result<(), anyhow::Error> {
    let args = Cli::parse();
    let paths = ResolvedPaths::resolve(args.vectors);
    paths.check_only_vectors_file_exists()?;

    let distributions = Distributions::load("./generation_settings.toml")?;

    let file = hdf5::File::open(paths.vectors_file)?;
    let nr_train = file.dataset("train")?.shape()[0];
    let nr_test = file.dataset("test")?.shape()[0];
    file.close()?;

    let rng = &mut distributions.create_rng();

    generate_payloads(nr_train, &paths.document_payload_file, "document", || {
        distributions.sample_document_payload(rng)
    })?;
    generate_payloads(nr_test, &paths.query_payload_file, "query", || {
        distributions.sample_query_payload(rng)
    })?;

    Ok(())
}

fn generate_payloads<S>(
    number: usize,
    file: &Path,
    hint: &str,
    mut genfn: impl FnMut() -> S,
) -> Result<(), Error>
where
    S: Serialize,
{
    eprintln!("Starting {hint} payload generation (x{number})");
    let payloads: Vec<_> = (0..number).map(|_| genfn()).collect();

    eprintln!("Writing {hint} payload file");
    dump_bincode(file, &payloads)?;
    Ok(())
}
