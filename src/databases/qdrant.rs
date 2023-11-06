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

use anyhow::{anyhow, bail, Error};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use qdrant_client::{
    prelude::QdrantClient,
    qdrant::{
        condition::ConditionOneOf, point_id::PointIdOptions, quantization_config::Quantization,
        r#match::MatchValue, read_consistency, value::Kind, vectors::VectorsOptions,
        vectors_config, with_payload_selector, CollectionStatus, Condition, CreateCollection,
        Distance, FieldCondition, Filter, HnswConfigDiff, ListValue, Match, OptimizersConfigDiff,
        PointId, PointStruct, QuantizationConfig, QuantizationType, Range, ReadConsistency,
        ReadConsistencyType, RepeatedStrings, ScalarQuantization, SearchParams, SearchPoints,
        Value, Vector, VectorParams, Vectors, VectorsConfig, WithPayloadSelector,
    },
};
use tokio::time::sleep;
use uuid::Uuid;

use crate::{
    benchmarks::QueryVectorDatabase,
    distribution::{
        ingestion::{IngestionInfo, PrepareVectorDatabase},
        DateFilter, LabelFilter, Labels, QueryPayload,
    },
};

pub struct Qdrant {
    client: QdrantClient,
    collection: String,
    vector_size: u64,
}

impl Qdrant {
    pub fn new(node_id: u16) -> Result<Self, Error> {
        if node_id == 0 || node_id > 9 {
            bail!("only support node id [1;9]");
        }
        let port = 6400 + node_id * 10 + 4;

        Ok(Qdrant {
            client: QdrantClient::from_url(&format!("http://localhost:{port}")).build()?,
            collection: "content".into(),
            vector_size: 960,
        })
    }

    async fn is_fully_ready(&self) -> Result<bool, Error> {
        let status = self
            .client
            .collection_info(&self.collection)
            .await?
            .result
            .ok_or_else(|| anyhow!("missing result"))?
            .status;
        //TODO recently it was green but still high cpu load??
        Ok(status == CollectionStatus::Green as i32)
    }
}

fn qdrant_labels(labels: &Labels) -> Value {
    Value {
        kind: Some(Kind::ListValue(ListValue {
            values: labels
                .iter()
                .map(|author| Value {
                    kind: Some(Kind::StringValue(author.to_string())),
                })
                .collect(),
        })),
    }
}

fn qdrant_time(time: DateTime<Utc>) -> Value {
    Value {
        kind: Some(Kind::IntegerValue(time.timestamp())),
    }
}

fn qdrant_vector(data: impl Into<Vec<f32>>) -> Vectors {
    Vectors {
        vectors_options: Some(VectorsOptions::Vector(Vector { data: data.into() })),
    }
}

#[async_trait(?Send)]
impl PrepareVectorDatabase for Qdrant {
    async fn initialize(&self) -> Result<bool, Error> {
        let needs_ingestion = if self.client.has_collection(&self.collection).await? {
            let info = self.client.collection_info(&self.collection).await?;
            info.result
                .map(|info| info.vectors_count == 0)
                .unwrap_or(true)
        } else {
            self.client
                .create_collection(&CreateCollection {
                    collection_name: self.collection.clone(),
                    hnsw_config: Some(HnswConfigDiff {
                        m: Some(16),
                        ef_construct: Some(100),
                        on_disk: Some(false),
                        ..HnswConfigDiff::default()
                    }),
                    optimizers_config: Some(OptimizersConfigDiff {
                        memmap_threshold: Some(6_000_000),
                        ..OptimizersConfigDiff::default()
                    }),
                    shard_number: Some(3),
                    replication_factor: Some(1),
                    vectors_config: Some(VectorsConfig {
                        config: Some(vectors_config::Config::Params(VectorParams {
                            //TODO parameterize
                            size: self.vector_size,
                            distance: Distance::Euclid as _,
                            ..VectorParams::default()
                        })),
                    }),
                    quantization_config: Some(QuantizationConfig {
                        quantization: Some(Quantization::Scalar(ScalarQuantization {
                            r#type: QuantizationType::Int8 as _,
                            ..ScalarQuantization::default()
                        })),
                    }),
                    ..CreateCollection::default()
                })
                .await?;

            true
        };

        Ok(needs_ingestion)
    }

    async fn prepare_mass_ingestion(&self) -> Result<(), Error> {
        self.client
            .update_collection(
                &self.collection,
                &OptimizersConfigDiff {
                    //disable indexing
                    indexing_threshold: Some(0),
                    ..Default::default()
                },
            )
            .await?;
        Ok(())
    }

    async fn finish_mass_ingestion(&self, target_max_time: Duration) -> Result<(), Error> {
        self.client
            .update_collection(
                &self.collection,
                &OptimizersConfigDiff {
                    //reset to default value
                    indexing_threshold: Some(20_000),
                    ..Default::default()
                },
            )
            .await?;

        let start = Instant::now();
        let mut ready_count = 0;
        while Instant::now().duration_since(start) < target_max_time {
            //Hint: It seems to be flacky
            if self.is_fully_ready().await? {
                ready_count += 1;
                if ready_count >= 3 {
                    return Ok(());
                }
            } else {
                ready_count = 0;
            }
            sleep(Duration::from_secs(1)).await;
        }
        bail!(
            "failed to finish mass ingestion even after {:.2}s",
            Instant::now().duration_since(start).as_secs_f32()
        )
    }

    async fn ingest_batch(
        &self,
        batch: impl IntoIterator<Item = IngestionInfo<'_>>,
    ) -> Result<(), Error> {
        self.client
            .upsert_points(
                &self.collection,
                batch
                    .into_iter()
                    .map(|info| PointStruct {
                        id: Some(PointId {
                            point_id_options: Some(PointIdOptions::Uuid(info.id.to_string())),
                        }),
                        payload: [
                            (
                                "publication_date".to_owned(),
                                qdrant_time(info.payload.publication_date),
                            ),
                            ("authors".to_owned(), qdrant_labels(&info.payload.authors)),
                            ("tags".to_owned(), qdrant_labels(&info.payload.tags)),
                            ("link".to_owned(), info.payload.link.as_str().into()),
                        ]
                        .into(),
                        vectors: Some(qdrant_vector(info.vector)),
                    })
                    .collect(),
                None,
            )
            .await?;

        Ok(())
    }
}

#[async_trait]
impl QueryVectorDatabase for Qdrant {
    fn name(&self) -> &str {
        "qdrant"
    }

    async fn query(
        &self,
        k: usize,
        ef: usize,
        vector: &[f32],
        payload: &QueryPayload,
        return_payload: bool,
    ) -> Result<Vec<Uuid>, Error> {
        let result = self
            .client
            .search_points(&SearchPoints {
                collection_name: self.collection.clone(),
                vector: vector.into(),
                filter: qdrant_filter(payload),
                limit: k as _,
                params: Some(SearchParams {
                    hnsw_ef: Some(ef as _),
                    ..SearchParams::default()
                }),
                with_payload: Some(WithPayloadSelector {
                    selector_options: Some(with_payload_selector::SelectorOptions::Enable(
                        return_payload,
                    )),
                }),
                read_consistency: Some(ReadConsistency {
                    value: Some(read_consistency::Value::Type(
                        ReadConsistencyType::Quorum as _,
                    )),
                }),
                ..SearchPoints::default()
            })
            .await?;

        result
            .result
            .into_iter()
            .map(|entry| {
                let Some(PointId {
                    point_id_options: Some(PointIdOptions::Uuid(uuid)),
                }) = entry.id
                else {
                    bail!("document without uuid: {:?}", entry);
                };
                Ok(uuid.parse()?)
            })
            .collect()
    }
}

fn qdrant_filter(
    QueryPayload {
        publication_date,
        authors,
        tags,
    }: &QueryPayload,
) -> Option<Filter> {
    qdrant_merge_filter_all_of([
        qdrant_date_filter("publication_date", publication_date),
        qdrant_label_filter("authors", authors),
        qdrant_label_filter("tags", tags),
    ])
}

fn qdrant_label_filter(field: &str, LabelFilter { include, exclude }: &LabelFilter) -> Filter {
    Filter {
        should: Vec::new(),
        must: qdrant_match_any_labels_condition(field, include)
            .map(|condition| vec![condition])
            .unwrap_or_default(),
        must_not: qdrant_match_any_labels_condition(field, exclude)
            .map(|condition| vec![condition])
            .unwrap_or_default(),
    }
}

fn qdrant_match_any_labels_condition(field: &str, labels: &Labels) -> Option<Condition> {
    if labels.is_empty() {
        None
    } else {
        Some(Condition {
            condition_one_of: Some(ConditionOneOf::Field(FieldCondition {
                key: field.into(),
                r#match: Some(Match {
                    match_value: Some(MatchValue::Keywords(RepeatedStrings {
                        // Hint: must_not match any of <strings>
                        strings: labels.to_uuid_string_vec(),
                    })),
                }),
                ..FieldCondition::default()
            })),
        })
    }
}

fn qdrant_date_filter(
    field: &str,
    DateFilter {
        lower_bound,
        upper_bound,
    }: &DateFilter,
) -> Filter {
    let date_range = (lower_bound.is_some() || upper_bound.is_some()).then(|| Condition {
        condition_one_of: Some(ConditionOneOf::Field(FieldCondition {
            key: field.into(),
            r#match: None,
            range: Some(Range {
                lt: None,
                gt: None,
                gte: lower_bound.map(|bound| bound.timestamp() as _),
                lte: upper_bound.map(|bound| bound.timestamp() as _),
            }),
            ..FieldCondition::default()
        })),
    });

    Filter {
        should: Vec::new(),
        must: date_range.map(|range| vec![range]).unwrap_or_default(),
        must_not: Vec::new(),
    }
}

fn qdrant_merge_filter_all_of(filters: impl IntoIterator<Item = Filter>) -> Option<Filter> {
    let filter = filters.into_iter().fold(
        Filter {
            should: Vec::new(),
            must: Vec::new(),
            must_not: Vec::new(),
        },
        |mut merged, filter| {
            merged.must.extend(filter.must);
            merged.must_not.extend(filter.must_not);
            merged.should.extend(filter.should);
            merged
        },
    );

    (!(filter.should.is_empty() && filter.must.is_empty() && filter.must_not.is_empty()))
        .then_some(filter)
}
