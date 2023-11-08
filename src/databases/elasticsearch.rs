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
use async_trait::async_trait;
use derive_more::{Deref, DerefMut};
use reqwest::{Client, Method, Response, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use url::Url;
use uuid::Uuid;

use crate::{
    benchmarks::QueryVectorDatabase,
    distribution::{
        ingestion::{IngestionInfo, PrepareVectorDatabase},
        DateFilter, LabelFilter, Labels, QueryPayload,
    },
    utils::{await_and_check_request, body_to_error},
};

pub struct Elasticsearch {
    client: Client,
    base_url: Url,
    index: String,
}

impl Elasticsearch {
    pub fn new() -> Result<Elasticsearch, Error> {
        Ok(Self {
            client: Client::new(),
            base_url: "http://localhost:9200/".parse()?,
            index: "content".into(),
        })
    }

    fn make_url(&self, segments: impl IntoIterator<Item = impl AsRef<str>>) -> Url {
        let mut url = self.base_url.clone();
        url.path_segments_mut()
            .unwrap()
            .extend([&self.index])
            .extend(segments);
        url
    }

    async fn json_request(
        &self,
        method: Method,
        segments: impl IntoIterator<Item = impl AsRef<str>>,
        data: &impl Serialize,
    ) -> Result<Response, Error> {
        let fut = self
            .client
            .request(method, self.make_url(segments))
            .json(data)
            .send();

        await_and_check_request(fut).await
    }
}

#[async_trait(?Send)]
impl PrepareVectorDatabase for Elasticsearch {
    async fn initialize(&self) -> Result<bool, Error> {
        let response = self
            .client
            .get(self.make_url([] as [&str; 0]))
            .send()
            .await?;

        let status = response.status();
        if status == StatusCode::OK {
            Ok(false)
        } else if status == StatusCode::NOT_FOUND {
            self.json_request(
                Method::PUT,
                [] as [&str; 0],
                &json!({
                    "settings": {
                        "index": {
                            "number_of_shards": 3,
                            "number_of_replicas": 1,
                        },
                    },
                    "mappings": {
                        "dynamic": "strict",
                        "properties": {
                            "embedding": {
                                "type": "dense_vector",
                                "dims": 960,
                                "index": true,
                                "element_type": "float",
                                "similarity": "l2_norm",
                                "index_options": {
                                    "type": "hnsw",
                                    "m": 16,
                                    "ef_construction": 100,
                                }
                            },
                            "publication_date": {
                                // for simplicity timestamp instead of proper date
                                "type": "long",
                            },
                            "authors": {
                                "type": "keyword"
                            },
                            "tags": {
                                "type": "keyword"
                            },
                            "link": {
                                "type": "keyword"
                            },
                        },
                    },
                }),
            )
            .await?;
            Ok(true)
        } else {
            Err(body_to_error(response).await)
        }
    }

    async fn prepare_mass_ingestion(&self) -> Result<(), Error> {
        // nothing to do
        Ok(())
    }

    async fn finish_mass_ingestion(&self, target_max_time: Duration) -> Result<(), Error> {
        let mut url = self.base_url.clone();
        url.path_segments_mut()
            .unwrap()
            .extend(["_cluster", "health"]);
        url.query_pairs_mut()
            .append_pair("wait_for_status", "green")
            .append_pair("timeout", &format!("{}s", target_max_time.as_secs()));
        await_and_check_request(self.client.get(url).send()).await?;
        Ok(())
    }

    async fn ingest_batch(
        &self,
        batch: impl IntoIterator<Item = IngestionInfo<'_>>,
    ) -> Result<(), Error> {
        let mut body = Vec::new();

        for IngestionInfo {
            id,
            vector,
            payload,
        } in batch
        {
            serde_json::to_writer(
                &mut body,
                &json!({
                    "index": { "_id": id.to_string() },
                }),
            )?;
            body.push(b'\n');
            serde_json::to_writer(
                &mut body,
                &json!({
                    "embedding": &vector,
                    "publication_date": payload.publication_date.timestamp(),
                    "authors": payload.authors.to_uuid_string_vec(),
                    "tags": payload.tags.to_uuid_string_vec(),
                    "link": &payload.link,
                }),
            )?;
            body.push(b'\n');
        }

        let fut = self
            .client
            .post(self.make_url(["_bulk"]))
            .header("Content-Type", "application/x-ndjson")
            .body(body)
            .send();

        await_and_check_request(fut).await?;

        Ok(())
    }
}

#[async_trait]
impl QueryVectorDatabase for Elasticsearch {
    fn name(&self) -> &str {
        "elasticsearch"
    }

    async fn query(
        &self,
        k: usize,
        ef: usize,
        vector: &[f32],
        payload: &QueryPayload,
        return_payload: bool,
        use_filters: bool,
    ) -> Result<Vec<Uuid>, Error> {
        let mut results = self
            .json_request(
                Method::POST,
                ["_search"],
                &ElasticQuery::new(k, ef, vector, payload, return_payload, use_filters),
            )
            .await?
            .json::<SearchResult>()
            .await?
            .hits
            .hits;

        results.sort_by(|l, r| l.score.total_cmp(&r.score));

        Ok(results.into_iter().map(|hit| hit.id).collect())
    }
}

#[derive(Deserialize)]
struct SearchResult {
    hits: Hits,
}

#[derive(Deserialize)]
struct Hits {
    hits: Vec<Hit>,
}

#[derive(Deserialize)]
struct Hit {
    #[serde(rename = "_id")]
    id: Uuid,
    #[serde(rename = "_score")]
    score: f32,
}

#[derive(Serialize)]
struct ElasticQuery<'a> {
    knn: KnnQuery<'a>,
    #[serde(rename = "_source")]
    return_payload: bool,
}

impl<'a> ElasticQuery<'a> {
    fn new(
        k: usize,
        ef: usize,
        vector: &'a [f32],
        payload: &QueryPayload,
        return_payload: bool,
        use_filters: bool,
    ) -> Self {
        Self {
            knn: KnnQuery {
                field: "embedding",
                query_vector: vector,
                k,
                //WARNING: This isn't exactly the same as `ef`, but the closest thing to `ef` we get.
                num_candidates: ef,
                filter: use_filters
                    .then(|| {
                        BoolQuery::default()
                            .with_date_range_filter("publication_date", &payload.publication_date)
                            .with_label_filter("authors", &payload.authors)
                            .with_label_filter("tags", &payload.tags)
                            .into_option()
                    })
                    .flatten(),
            },
            return_payload,
        }
    }
}

#[derive(Serialize)]
struct KnnQuery<'a> {
    field: &'a str,
    query_vector: &'a [f32],
    k: usize,
    num_candidates: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    filter: Option<BoolQuery>,
}

#[derive(Default, Deref, DerefMut, Serialize)]
struct BoolQuery {
    bool: BoolQueryInner,
}

impl BoolQuery {
    fn into_option(self) -> Option<Self> {
        (!self.filter.is_empty() || !self.must_not.is_empty()).then(|| self)
    }
}

#[derive(Default, Serialize)]
struct BoolQueryInner {
    filter: Vec<Value>,
    must_not: Vec<Value>,
}

impl BoolQuery {
    fn with_date_range_filter(mut self, field: &str, input: &DateFilter) -> Self {
        let mut range_constraints = Map::new();
        if let Some(lower_bound) = input.lower_bound {
            range_constraints.insert("gte".to_owned(), lower_bound.timestamp().into());
        }
        if let Some(upper_bound) = input.upper_bound {
            range_constraints.insert("lte".to_owned(), upper_bound.timestamp().into());
        }

        if !range_constraints.is_empty() {
            self.filter.push(json!({
                "range": {
                    field: range_constraints,
                }
            }))
        }
        self
    }

    fn with_label_filter(mut self, field: &str, input: &LabelFilter) -> Self {
        elastic_match_keyword_constraints(field, &input.include, &mut self.filter);
        elastic_match_keyword_constraints(field, &input.exclude, &mut self.must_not);
        self
    }
}

fn elastic_match_keyword_constraints(field: &str, keywords: &Labels, out: &mut Vec<Value>) {
    if !keywords.is_empty() {
        out.push(json!({
            "terms": {
                field: keywords.to_uuid_string_vec(),
            }
        }))
    }
}
