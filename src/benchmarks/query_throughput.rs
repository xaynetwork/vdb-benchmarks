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

use anyhow::Error;
use criterion::{measurement::Measurement, BenchmarkGroup, BenchmarkId, Criterion, Throughput};
use rand::{rngs::StdRng, thread_rng, Rng, SeedableRng};
use rand_distr::Uniform;
use tokio::{runtime::Runtime, task::JoinSet};

use crate::{
    benchmarks::{IngestionParameters, QueryParameters},
    distribution::QueryPayload,
    resources::{load_bincode, load_vectors, ResolvedPaths},
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
    paths: &ResolvedPaths,
    database: impl QueryVectorDatabase,
    c: &mut Criterion,
) -> Result<(), Error> {
    paths.check_files_exists()?;

    let payloads: Vec<QueryPayload> = load_bincode(&paths.query_payload_file)?;
    let vectors = load_vectors(&paths.vectors_file, "test")?;
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

    for k in [20, 50, 80, 100] {
        bench(
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
            },
        );
        if k == 100 {
            bench(
                &mut group,
                rt,
                &inputs,
                ingestion_parameters,
                QueryParameters {
                    k,
                    fetch_payload: true,
                    ef: k,
                    number_of_tasks: 5,
                    queries_per_task: 10,
                },
            );
        }
    }

    Ok(())
}

fn bench<M, DB>(
    group: &mut BenchmarkGroup<'_, M>,
    rt: &Runtime,
    inputs: &Arc<Inputs<DB>>,
    iparams: IngestionParameters,
    qparams: QueryParameters,
) where
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
    } = qparams;
    assert!(k <= ef);
    assert!(queries_per_task > 0);
    assert!(number_of_tasks > 0);
    group
        .throughput(Throughput::Elements(
            (queries_per_task * number_of_tasks) as _,
        ))
        .bench_with_input(
            BenchmarkId::new("query_throughput", format!("{iparams},{qparams}")),
            inputs,
            |b, inputs| {
                b.to_async(rt).iter(|| async {
                    let mut tasks = JoinSet::<Result<(), Error>>::new();
                    for _ in 0..number_of_tasks {
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
                                database
                                    .query(&vectors[idx], &payloads[idx], fetch_payload)
                                    .await?;
                            }
                            Ok(())
                        });
                    }

                    while let Some(result) = tasks.join_next().await {
                        result.unwrap().unwrap();
                    }
                })
            },
        );
}
