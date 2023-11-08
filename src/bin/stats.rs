use std::{
    collections::HashSet,
    fs::{self, File},
    io::{BufRead, BufReader, BufWriter, Write},
    path::Path,
};

use anyhow::{anyhow, Error};
use serde::Deserialize;
use serde_json::json;
use vdb_benchmarks::math::WelfordOnlineAlgorithm;
use walkdir::WalkDir;

//TODO allow configuring dirs
fn main() -> Result<(), anyhow::Error> {
    //TODO do not hardcode this
    let neighbors = load_expected_neighbors("./resources/gist-960-euclidean.hdf5")?;
    for entry in WalkDir::new("./reports/additional_data") {
        let entry = entry?;
        let recall_file = entry.path().with_file_name("recall.json");
        if entry.file_name() != "recall_data.jsonl" {
            continue;
        } else if recall_file.exists() {
            let data = String::from_utf8(fs::read(&recall_file)?)?;
            println!("Stats: {}: {data}", recall_file.display());
            continue;
        }

        let mut lines = BufReader::new(File::open(entry.path())?).lines();
        let Header { expected_hits } = serde_json::from_str(dbg!(lines
            .next()
            .ok_or_else(|| anyhow!("empty recall_data.json"))??
            .trim()))?;

        let mut precision = WelfordOnlineAlgorithm::new();
        let mut recall = WelfordOnlineAlgorithm::new();

        for line in lines {
            let data: Vec<(usize, HashSet<usize>)> = serde_json::from_str(line?.trim())?;
            for (idx, got_neighbors) in data {
                let expected_neighbors = &neighbors[idx];
                let true_positive = got_neighbors.intersection(&expected_neighbors).count() as f64;
                precision.update(if true_positive == 0. {
                    0.
                } else {
                    true_positive / got_neighbors.len() as f64
                });
                recall.update(true_positive / expected_hits as f64);
            }
        }

        let mut out = BufWriter::new(
            File::options()
                .create_new(true)
                .write(true)
                .open(&recall_file)?,
        );
        let data = json!({
            "precision": precision,
            "recall": recall,
        });
        println!("Stats: {}: {data}", recall_file.display());
        serde_json::to_writer(&mut out, &data)?;
        out.flush()?;
    }

    Ok(())
}

fn load_expected_neighbors(vectors: impl AsRef<Path>) -> Result<Vec<HashSet<usize>>, Error> {
    let file = hdf5::File::open(vectors)?;
    let dataset = file.dataset("neighbors")?;
    let neighbors = (0..dataset.shape()[0])
        .map(|idx| match dataset.read_slice_1d(ndarray::s![idx, ..]) {
            Ok(array) => Ok(array.iter().copied().collect()),
            Err(err) => Err(anyhow!("malformed vector dataset: {err}")),
        })
        .collect::<Result<Vec<_>, _>>()?;
    file.close()?;
    Ok(neighbors)
}

#[derive(Deserialize)]
struct Header {
    expected_hits: usize,
}
