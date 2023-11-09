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

use std::sync::Arc;

use anyhow::{Context, Error};
use criterion::{measurement::Measurement, BenchmarkGroup, BenchmarkId, Criterion, Throughput};
use rand::{rngs::StdRng, thread_rng, Rng, SeedableRng};
use rand_distr::Uniform;
use serde_json::json;
use tokio::{runtime::Runtime, sync::mpsc, task::JoinSet};
use uuid::Uuid;

use crate::{
    benchmarks::{IngestionParameters, QueryParameters},
    consts::{DOCKER_LIMIT_CPUS, DOCKER_LIMIT_MEMORY},
    distribution::{ids::fake_uuid_to_index, QueryPayload},
    docker::DockerStatScanner,
    resources::{load_bincode, load_vectors, ResolvedPaths, ResourceWriter},
};

use super::QueryVectorDatabase;

struct Inputs<DB>
where
    DB: QueryVectorDatabase,
{
    database: DB,
    payloads: Vec<QueryPayload>,
    vectors: Vec<Vec<f32>>,
}

pub fn benchmark(
    writer: &ResourceWriter,
    paths: &ResolvedPaths,
    database: impl QueryVectorDatabase,
    c: &mut Criterion,
) -> Result<(), Error> {
    paths.check_files_exists()?;

    let cpus = *DOCKER_LIMIT_CPUS;
    let mem_limit = *DOCKER_LIMIT_MEMORY;

    let writer = &writer.sub_writer("query_throughput")?;
    writer.write_file("path.json", paths)?;

    let payloads: Vec<QueryPayload> =
        load_bincode(&paths.query_payload_file).context("loading bincode payloads")?;
    let vectors = load_vectors(&paths.vectors_file, "test").context("loading vector data")?;
    let nr_elements = vectors.len();
    assert!(payloads.len() == nr_elements);

    let inputs = Arc::new(Inputs {
        database,
        payloads,
        vectors,
    });

    //TODO don't hardcode this
    let ingestion_parameters = IngestionParameters {
        m: 16,
        ef_construct: 100,
    };

    let rt = &Runtime::new()?;
    let mut group = c.benchmark_group(inputs.database.name());

    bench(
        writer,
        &mut group,
        rt,
        &inputs,
        ingestion_parameters,
        QueryParameters {
            k: 10,
            ef: 10,
            number_of_tasks: 5,
            queries_per_task: 10,
            fetch_payload: false,
            use_filters: false,
            cpus,
            mem_limit,
        },
    )?;

    bench(
        writer,
        &mut group,
        rt,
        &inputs,
        ingestion_parameters,
        QueryParameters {
            k: 10,
            ef: 200,
            number_of_tasks: 5,
            queries_per_task: 10,
            fetch_payload: false,
            use_filters: false,
            cpus,
            mem_limit,
        },
    )?;

    for k in [10, 50, 100] {
        bench(
            writer,
            &mut group,
            rt,
            &inputs,
            ingestion_parameters,
            QueryParameters {
                k,
                fetch_payload: false,
                ef: k,
                number_of_tasks: 5,
                queries_per_task: 10,
                use_filters: true,
                cpus,
                mem_limit,
            },
        )?;
    }

    bench(
        writer,
        &mut group,
        rt,
        &inputs,
        ingestion_parameters,
        QueryParameters {
            k: 100,
            fetch_payload: true,
            ef: 100,
            number_of_tasks: 5,
            queries_per_task: 10,
            use_filters: true,
            cpus,
            mem_limit,
        },
    )?;

    bench(
        writer,
        &mut group,
        rt,
        &inputs,
        ingestion_parameters,
        QueryParameters {
            k: 10,
            fetch_payload: false,
            ef: 100,
            number_of_tasks: 5,
            queries_per_task: 10,
            use_filters: true,
            cpus,
            mem_limit,
        },
    )?;

    // with given resource limits we have to be careful to not
    // go too high as it will timeout (it's non stop 5/10/20 requests
    // not 5/10/20 hy\pothetical users)
    for number_of_tasks in [5, 10, 20] {
        bench(
            writer,
            &mut group,
            rt,
            &inputs,
            ingestion_parameters,
            QueryParameters {
                k: 10,
                fetch_payload: false,
                ef: 100,
                number_of_tasks,
                queries_per_task: 1,
                use_filters: true,
                cpus,
                mem_limit,
            },
        )?;
    }

    for k in [5, 25, 50] {
        bench(
            writer,
            &mut group,
            rt,
            &inputs,
            ingestion_parameters,
            QueryParameters {
                k,
                fetch_payload: false,
                ef: 50,
                number_of_tasks: 5,
                queries_per_task: 10,
                use_filters: true,
                cpus,
                mem_limit,
            },
        )?;
    }

    Ok(())
}

fn bench<M, DB>(
    writer: &ResourceWriter,
    group: &mut BenchmarkGroup<'_, M>,
    rt: &Runtime,
    inputs: &Arc<Inputs<DB>>,
    iparams: IngestionParameters,
    qparams: QueryParameters,
) -> Result<(), Error>
where
    DB: QueryVectorDatabase,
    M: Measurement + 'static,
    Arc<Inputs<DB>>: Send,
{
    let QueryParameters {
        k,
        ef,
        fetch_payload,
        number_of_tasks,
        queries_per_task,
        use_filters,
        cpus: _,
        mem_limit: _,
    } = qparams;
    //FIXME We currently can only have recall data for the non filter case
    //      for now, but running a perfect KNN with filters would be nice.
    let enable_recall_stats = !use_filters;
    assert!(k <= ef);
    assert!(queries_per_task > 0);
    assert!(number_of_tasks > 0);

    let bench_id = format!("{iparams}_{qparams}");
    let writer = writer.sub_writer(&bench_id)?;
    let writer2 = writer.clone();
    let docker_stats = DockerStatScanner::start(rt.handle(), inputs.database.name())?;

    // We send the recall data out of the benchmark and write it in a separate task.

    let (recall_sender, mut recall_receiver) = mpsc::unbounded_channel();
    let writer_task = rt.spawn(async move {
        if !enable_recall_stats {
            recall_receiver.close();
            return Ok(());
        }

        let recall_file = "recall_data.jsonl";
        writer.write_file(
            recall_file,
            &json!({
                "expected_hits": k,
            }),
        )?;
        let mut recall_data = Vec::new();
        while let Some(group) = recall_receiver.recv().await {
            match group {
                Group::Add { query_id, vectors } => recall_data.push((
                    query_id,
                    vectors
                        .into_iter()
                        .map(fake_uuid_to_index)
                        .collect::<Vec<_>>(),
                )),
                Group::Write => {
                    writer.append_line_to_file(recall_file, &recall_data)?;
                    recall_data.truncate(0);
                }
            }
        }
        Result::<_, Error>::Ok(())
    });

    group
        .throughput(Throughput::Elements(
            (queries_per_task * number_of_tasks) as _,
        ))
        .bench_with_input(
            BenchmarkId::new("query_throughput", bench_id),
            inputs,
            move |b, inputs| {
                b.to_async(rt).iter(|| async {
                    let mut tasks = JoinSet::<Result<(), Error>>::new();
                    for _ in 0..number_of_tasks {
                        let recall_sender = recall_sender.clone();
                        let inputs = inputs.clone();
                        tasks.spawn(async move {
                            let Inputs {
                                database,
                                payloads,
                                vectors,
                            } = &*inputs;
                            // we randomly sample queries from the set of test queries
                            let rng = StdRng::from_rng(thread_rng())?;
                            for idx in rng
                                .sample_iter(Uniform::new(0, inputs.vectors.len()))
                                .take(queries_per_task)
                            {
                                let vectors = database
                                    .query(
                                        k,
                                        ef,
                                        &vectors[idx],
                                        &payloads[idx],
                                        fetch_payload,
                                        use_filters,
                                    )
                                    .await?;

                                if enable_recall_stats {
                                    recall_sender.send(Group::Add {
                                        query_id: idx,
                                        vectors,
                                    })?;
                                }
                            }
                            Ok(())
                        });
                    }

                    while let Some(result) = tasks.join_next().await {
                        result.unwrap().unwrap();
                    }

                    if enable_recall_stats {
                        recall_sender.send(Group::Write).unwrap();
                    }
                })
            },
        );

    let stats = rt.block_on(docker_stats.stop())?;
    writer2.write_file("docker_stats.json", &stats)?;
    rt.block_on(writer_task)??;
    Ok(())
}

enum Group {
    Add { query_id: usize, vectors: Vec<Uuid> },
    Write,
}
