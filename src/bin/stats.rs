use std::{
    fs::{self, File},
    io::{BufRead, BufReader, BufWriter, Write},
    path::{Path, PathBuf},
};

use anyhow::{anyhow, bail, Context, Error};
use clap::Parser;
use serde::{Deserialize, Serialize};
use vdb_benchmarks::{docker::DockerStats, math::WelfordOnlineAlgorithm};

#[derive(Parser, Debug)]
#[command(version)]
struct Cli {
    /// The report dir either `./reports` or `./committed_reports/<reports_name>/`
    #[arg(index = 1)]
    report_dir: PathBuf,
}

//TODO allow configuring dirs
fn main() -> Result<(), anyhow::Error> {
    let cli = Cli::parse();
    //TODO do not hardcode this
    let neighbors = load_expected_neighbors("./resources/gist-960-euclidean.hdf5")?;

    let data_dir = cli.report_dir.join("additional_data");
    if !data_dir.exists() {
        bail!(
            "report dir should contain the ./additional_data sub-dir: {}",
            data_dir.display()
        );
    }

    visit_sub_dirs(data_dir, |provider, provider_path| {
        visit_sub_dirs(provider_path, |run, run_path| {
            visit_sub_dirs(run_path, |bench_group, bench_group_path| {
                if bench_group == "ingestion" {
                    return Ok(());
                }
                visit_sub_dirs(bench_group_path, |bench_id, bench_path| {
                    println!("{run} {provider}/{bench_group}/{bench_id}");
                    if let Some(recall) =
                        retrieve_recall(&neighbors, bench_path).context("retrieve_recall")?
                    {
                        println!("{run} recall@10    {:.4}", recall.recall.mean());
                        println!("{run} precision@10 {:.4}", recall.precision.mean())
                    }
                    if let Some(docker_stats) =
                        retrieve_docker_stats(bench_path).context("retrieve_docker_stats")?
                    {
                        println!(
                            "{run} cpu       {mean: >5.0} / {max: >5.0} / {std: >5.0}",
                            mean = docker_stats.cpu.mean(),
                            max = docker_stats.cpu.max(),
                            std = docker_stats.cpu.sample_std(),
                        );
                        println!(
                            "{run} memory    {mean: >5.2} / {max: >5.2} / {std: >5.2}",
                            mean = docker_stats.memory.mean(),
                            max = docker_stats.memory.max(),
                            std = docker_stats.memory.sample_std(),
                        );
                    }
                    println!("{run} --");
                    Ok(())
                })
            })
        })
    })?;

    Ok(())
}

fn retrieve_docker_stats(bench_path: &Path) -> Result<Option<DockerStats>, Error> {
    let docker_file = bench_path.join("docker_stats.json");
    docker_file
        .exists()
        .then(|| {
            let reader = BufReader::new(File::open(docker_file)?);
            Ok(serde_json::from_reader(reader)?)
        })
        .transpose()
}

fn retrieve_recall(
    neighbors: &[Vec<usize>],
    bench_path: &Path,
) -> Result<Option<RecallAndPrecision>, Error> {
    let recall_data_file = bench_path.join("recall_data.jsonl");
    if !recall_data_file.exists() {
        return Ok(None);
    }

    let recall_file = bench_path.join("recall.json");
    let stats;
    if recall_file.exists() {
        let reader = BufReader::new(File::open(recall_file)?);
        stats = serde_json::from_reader(reader)?;
    } else {
        let file = BufReader::new(File::open(recall_data_file)?);
        stats = calculate_stats(file, neighbors)?;
        let mut out = BufWriter::new(
            File::options()
                .create_new(true)
                .write(true)
                .open(&recall_file)?,
        );
        serde_json::to_writer(&mut out, &stats)?;
        out.flush()?;
    }

    Ok(Some(stats))
}

fn calculate_stats(
    source: impl BufRead,
    neighbors: &[Vec<usize>],
) -> Result<RecallAndPrecision, Error> {
    let mut lines = source.lines();
    let Header { expected_hits } = serde_json::from_str(
        lines
            .next()
            .ok_or_else(|| anyhow!("empty recall_data.json"))??
            .trim(),
    )?;

    let mut precision = WelfordOnlineAlgorithm::new();
    let mut recall = WelfordOnlineAlgorithm::new();

    for line in lines {
        let data: Vec<(usize, Vec<usize>)> = serde_json::from_str(line?.trim())?;
        for (idx, got_neighbors) in data {
            let expected_neighbors = &neighbors[idx][..10];
            let true_positive = got_neighbors
                .iter()
                .take(10)
                .filter(|got| expected_neighbors.contains(got))
                .count() as f64;
            precision.update(if true_positive == 0. {
                0.
            } else {
                true_positive / got_neighbors.len() as f64
            });
            recall.update(true_positive / expected_hits as f64);
        }
    }

    Ok(RecallAndPrecision { recall, precision })
}

#[derive(Serialize, Deserialize)]
struct RecallAndPrecision {
    recall: WelfordOnlineAlgorithm,
    precision: WelfordOnlineAlgorithm,
}

fn visit_sub_dirs(
    path: impl AsRef<Path>,
    mut visit: impl FnMut(&str, &Path) -> Result<(), Error>,
) -> Result<(), Error> {
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            let sub_path = entry.path();
            let name = entry.file_name();
            let name = name
                .to_str()
                .ok_or_else(|| anyhow!("non utf-8 path: {}", sub_path.display()))?;
            visit(name, &sub_path)?;
        }
    }
    Ok(())
}

fn load_expected_neighbors(vectors: impl AsRef<Path>) -> Result<Vec<Vec<usize>>, Error> {
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
