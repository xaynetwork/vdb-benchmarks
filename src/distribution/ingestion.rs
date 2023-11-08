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

use std::time::{Duration, Instant};

use anyhow::Error;
use async_trait::async_trait;
use serde_json::json;
use uuid::Uuid;

use crate::{
    distribution::{ids::index_to_fake_uuid, DocumentPayload},
    math::WelfordOnlineAlgorithm,
    resources::{load_bincode, load_vectors, ResolvedPaths, ResourceWriter},
};

pub struct IngestionInfo<'a> {
    pub id: Uuid,
    pub vector: &'a [f32],
    pub payload: &'a DocumentPayload,
}

#[async_trait(?Send)]
pub trait PrepareVectorDatabase {
    async fn initialize(&self) -> Result<bool, Error>;
    async fn prepare_mass_ingestion(&self) -> Result<(), Error>;
    async fn finish_mass_ingestion(&self, target_max_time: Duration) -> Result<(), Error>;
    async fn ingest_batch(
        &self,
        batch: impl IntoIterator<Item = IngestionInfo<'_>>,
    ) -> Result<(), Error>;
}

pub fn load_ingestion_data(
    paths: &ResolvedPaths,
) -> Result<(Vec<Vec<f32>>, Vec<DocumentPayload>), Error> {
    paths.check_files_exists()?;

    eprintln!("load data");
    let payloads: Vec<DocumentPayload> = load_bincode(&paths.document_payload_file)?;
    let vectors = load_vectors(&paths.vectors_file, "train")?;
    let nr_vectors = vectors.len();
    assert!(payloads.len() == nr_vectors);

    Ok((vectors, payloads))
}

const BATCH_SIZE: usize = 100;
pub async fn ingest_database(
    writer: &ResourceWriter,
    paths: &ResolvedPaths,
    database: &impl PrepareVectorDatabase,
) -> Result<(), Error> {
    let writer = writer.sub_writer("ingestion")?;
    writer.write_file("paths.json", paths)?;

    eprintln!("initialize database");
    let needs_ingestion = database.initialize().await?;
    if !needs_ingestion {
        return Ok(());
    }

    let (vectors, payloads) = load_ingestion_data(paths)?;
    let nr_documents = vectors.len();

    writer.write_file(
        "source.json",
        &json!({
            "dataset": paths.dataset_name()?,
            "documents": nr_documents,
            "vector_size":  vectors.get(0).map_or(0, Vec::len),
            "ingestion_batch_size": BATCH_SIZE,
        }),
    )?;

    let mut times = WelfordOnlineAlgorithm::new();

    let start = Instant::now();
    let _deferred = CallOnDrop::new(move || {
        let duration = Instant::now().duration_since(start).as_secs_f32();
        eprintln!("time spend ingesting: {duration:.2}s");
    });

    eprintln!("prepare ingestion");
    database.prepare_mass_ingestion().await?;
    let mut vectors = vectors.iter().zip(payloads.iter()).enumerate().peekable();
    eprintln!("ingestion started");
    let mut nr_ingested_entries = 0;
    while vectors.peek().is_some() {
        let batch_start = Instant::now();
        database
            .ingest_batch(
                vectors
                    .by_ref()
                    .take(BATCH_SIZE)
                    .map(|(idx, (vector, payload))| IngestionInfo {
                        id: index_to_fake_uuid(idx as _),
                        vector,
                        payload,
                    }),
            )
            .await?;
        nr_ingested_entries += BATCH_SIZE;
        let duration = Instant::now().duration_since(batch_start).as_secs_f64();
        times.update(duration);
        eprintln!(
            "progress: {:.2}%",
            nr_ingested_entries as f32 / nr_documents as f32 * 100.
        );
    }
    eprintln!("finish uploading, waiting for index to be ready");
    database
        .finish_mass_ingestion(Duration::from_secs(900))
        .await?;
    let duration = Instant::now().duration_since(start).as_secs_f32();
    eprintln!("full ingestion duration: {:.4}s", duration);

    writer.write_file(
        "times.json",
        &json!({
            "total": duration,
            "dist": times,
        }),
    )?;

    Ok(())
}

fn round_two_digits(input: f32) -> f32 {
    //WARNING: not numeric perfect
    (input * 100.).round() / 100.
}
struct CallOnDrop<F>
where
    F: FnOnce(),
{
    func: Option<F>,
}

impl<F> CallOnDrop<F>
where
    F: FnOnce(),
{
    fn new(func: F) -> Self {
        Self { func: Some(func) }
    }
}

impl<F> Drop for CallOnDrop<F>
where
    F: FnOnce(),
{
    fn drop(&mut self) {
        if let Some(func) = self.func.take() {
            func();
        }
    }
}
