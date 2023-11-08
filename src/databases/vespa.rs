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

use std::{cmp::min, collections::HashMap, fmt::Write, future::Future, time::Duration};

use anyhow::{anyhow, bail, Error};
use async_trait::async_trait;
use reqwest::{Client, Method, Response};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::task::JoinSet;
use url::Url;
use uuid::Uuid;

use crate::{
    benchmarks::QueryVectorDatabase,
    distribution::{
        ingestion::{IngestionInfo, PrepareVectorDatabase},
        DateFilter, LabelFilter, Labels, QueryPayload,
    },
    utils::await_and_check_request,
};

pub struct Vespa {
    client: Client,
    base_url: Url,
    namespace: String,
    document_type: String,
}

impl Vespa {
    pub fn new(node_id: u16) -> Result<Self, Error> {
        if node_id > 9 {
            bail!("port pattern only supports nodes [0;9]");
        }
        let port = 8080 + node_id;
        let namespace = "default".into();
        let document_type = "content".into();
        Ok(Vespa {
            client: Client::builder().http2_prior_knowledge().build()?,
            base_url: format!("http://localhost:{port}/").parse()?,
            namespace,
            document_type,
        })
    }

    fn make_url(&self, segments: impl IntoIterator<Item = impl AsRef<str>>) -> Url {
        let mut url = self.base_url.clone();
        url.path_segments_mut().unwrap().extend(segments);
        url
    }

    fn json_request(
        &self,
        method: Method,
        segments: impl IntoIterator<Item = impl AsRef<str>>,
        data: &impl Serialize,
    ) -> impl Future<Output = Result<Response, Error>> + Send + 'static {
        let fut = self
            .client
            .request(method, self.make_url(segments))
            .json(data)
            .send();

        async move { await_and_check_request(fut).await }
    }
}

#[async_trait]
impl QueryVectorDatabase for Vespa {
    fn name(&self) -> &str {
        "vespa"
    }

    async fn query(
        &self,
        k: usize,
        ef: usize,
        vector: &[f32],
        payload: &QueryPayload,
        return_payload: bool,
        use_filter: bool,
    ) -> Result<Vec<Uuid>, Error> {
        let query = yql_build_query(k, ef, vector, payload, return_payload, use_filter)?;
        let root = self
            //Hint: The trailing "" is important the path has to be /search/ not /search
            .json_request(Method::POST, ["search", ""], &query)
            .await?
            .json::<SearchResult>()
            .await?
            .root;

        let total_count = root.fields.total_count;
        let got_count = root.children.len();
        if min(total_count, k) != got_count {
            bail!("malformed result({total_count} != {got_count}): {root:?}");
        }

        root.children
            .into_iter()
            .map(|child| {
                child
                    .fields
                    .get("id")
                    .and_then(|o| o.as_str())
                    .ok_or_else(|| anyhow!("malformed or missing id field: {:?}", child))?
                    .parse()
                    .map_err(Error::from)
            })
            .collect()
    }
}

#[derive(Deserialize, Debug)]
struct SearchResult {
    root: Root,
}

#[derive(Deserialize, Debug)]
struct Root {
    #[serde(default)]
    children: Vec<Child>,
    fields: RootFields,
}

#[derive(Deserialize, Debug)]
struct RootFields {
    #[serde(rename = "totalCount")]
    total_count: usize,
}

#[derive(Deserialize, Debug)]
struct Child {
    // WARNING: we can't use this id it's not useful if the whole result is fetched from memory
    // id: String,
    fields: HashMap<String, Value>,
}

fn yql_build_query(
    k: usize,
    ef: usize,
    vector: &[f32],
    payload: &QueryPayload,
    return_payload: bool,
    use_filter: bool,
) -> Result<Value, Error> {
    let selector = if return_payload { " * " } else { " id " };
    let explore_additional_hits = ef - k;
    let mut query =
        format!("select{selector}from content where {{hnsw.exploreAdditionalHits:{explore_additional_hits}, targetHits:{k}}}nearestNeighbor(embedding, query_embedding)");

    if use_filter {
        yql_append_date_range(&mut query, "publication_date", &payload.publication_date)?;
        yql_append_label_filter(&mut query, "authors", &payload.authors)?;
        yql_append_label_filter(&mut query, "tags", &payload.tags)?;
    }

    Ok(json!({
        "yql": query,
        "ranking.profile": "ann",
        "input.query(query_embedding)": vector,
        "hits": k,
        // The default timeout is 0.5s, furthermore vespa will kill queries which seem doomed to timeout
        // early one, but we error and stop the benchmark once we hit a single error and with 100q being no
        // stop thrown at it in parallel (but over the same http2 client) and given resource limits some timeouts
        // are doomed to happen sooner or later. So we set them to the max value of 60s.
        "timeout": "60s",
    }))
}

//HINT: This is a terrible bad, but very easy to write down and good enough for our case approach to query building
const AND: &str = " and ";
const OR: &str = " or ";
fn yql_append_date_range(
    query: &mut String,
    field: &str,
    filter: &DateFilter,
) -> Result<(), Error> {
    if filter.lower_bound.is_some() || filter.upper_bound.is_some() {
        let lower_bound = filter
            .lower_bound
            .map_or_else(|| "-Infinity".into(), |bound| bound.timestamp().to_string());
        let upper_bound = filter
            .upper_bound
            .map_or_else(|| "Infinity".into(), |bound| bound.timestamp().to_string());
        write!(query, "{AND}range({field}, {lower_bound}, {upper_bound})")?;
    }
    Ok(())
}

fn yql_append_label_filter(
    query: &mut String,
    field: &str,
    filter: &LabelFilter,
) -> Result<(), Error> {
    yql_append_or_joined_labels(query, field, &filter.include, false)?;
    yql_append_or_joined_labels(query, field, &filter.exclude, true)?;
    Ok(())
}

const KEYWORD_EQ: &str = " contains ";
fn yql_append_or_joined_labels(
    query: &mut String,
    field: &str,
    labels: &Labels,
    negate_group: bool,
) -> Result<(), Error> {
    if !labels.is_empty() {
        query.push_str(AND);
        if negate_group {
            query.push('!');
        }
        query.push('(');
        let mut append_or = false;
        for label in &labels.0 {
            if append_or {
                query.push_str(OR);
            } else {
                append_or = true;
            }
            write!(query, "{field}{KEYWORD_EQ}{label:?}")?;
        }
        query.push(')');
    }
    Ok(())
}

#[async_trait(?Send)]
impl PrepareVectorDatabase for Vespa {
    async fn initialize(&self) -> Result<bool, Error> {
        //TODO check if empty, in the future maybe deploy tenant here instead of with docker helper
        Ok(true)
    }

    async fn prepare_mass_ingestion(&self) -> Result<(), Error> {
        // nothing to do here
        Ok(())
    }

    async fn finish_mass_ingestion(&self, _target_max_time: Duration) -> Result<(), Error> {
        // nothing to do here
        Ok(())
    }

    async fn ingest_batch(
        &self,
        batch: impl IntoIterator<Item = IngestionInfo<'_>>,
    ) -> Result<(), Error> {
        let mut tasks = JoinSet::new();

        for IngestionInfo {
            id,
            vector,
            payload,
        } in batch
        {
            tasks.spawn(self.json_request(
                Method::POST,
                [
                    "document",
                    "v1",
                    &self.namespace,
                    &self.document_type,
                    "docid",
                    &id.to_string(),
                ],
                &json!({
                    "fields": {
                        "id": id,
                        "embedding": vector,
                        "publication_date": payload.publication_date.timestamp(),
                        "authors": payload.authors.to_uuid_string_vec(),
                        "tags": payload.tags.to_uuid_string_vec(),
                        "link": &payload.link,
                    }
                }),
            ));
        }

        while let Some(result) = tasks.join_next().await {
            result??;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use crate::distribution::Label;

    use super::*;

    #[test]
    fn test_query_building() {
        let res = yql_build_query(
            10,
            22,
            &[2., 4., 6.],
            &QueryPayload {
                publication_date: DateFilter {
                    lower_bound: Some(Utc.with_ymd_and_hms(1970, 1, 1, 1, 1, 1).unwrap()),
                    upper_bound: Some(Utc.with_ymd_and_hms(2005, 2, 2, 2, 2, 2).unwrap()),
                },
                authors: LabelFilter {
                    include: Labels(vec![Label(12), Label(5)]),
                    exclude: Labels(vec![Label(3)]),
                },
                tags: LabelFilter {
                    include: Labels(vec![Label(7)]),
                    exclude: Labels(vec![Label(1), Label(321)]),
                },
            },
            true,
            true,
        )
        .unwrap();

        assert_eq!(
            res,
            json!({
                "yql": concat!(
                    "select * from content where",
                    " {hnsw.exploreAdditionalHits:12, targetHits:10}nearestNeighbor(embedding, query_embedding)",
                    " and range(publication_date, 3661, 1107309722)",
                    " and (authors contains \"00000000-0000-400c-8000-00000000000c\" or authors contains \"00000000-0000-4005-8000-000000000005\")",
                    " and !(authors contains \"00000000-0000-4003-8000-000000000003\")",
                    " and (tags contains \"00000000-0000-4007-8000-000000000007\")",
                    " and !(tags contains \"00000000-0000-4001-8000-000000000001\" or tags contains \"00000000-0000-4141-8000-000000000141\")"
                ),
                "input.query(query_embedding)": [2., 4., 6.],
                "ranking.profile": "ann",
                "hits": 10,
                "timeout": "60s",
            })
        );

        let res = yql_build_query(
            10,
            25,
            &[2., 4., 6.],
            &QueryPayload {
                publication_date: DateFilter {
                    lower_bound: Some(Utc.with_ymd_and_hms(1970, 1, 1, 1, 1, 1).unwrap()),
                    upper_bound: None,
                },
                authors: LabelFilter {
                    include: Labels::default(),
                    exclude: Labels(vec![Label(3)]),
                },
                tags: LabelFilter {
                    include: Labels(vec![Label(7)]),
                    exclude: Labels::default(),
                },
            },
            true,
            true,
        )
        .unwrap();

        assert_eq!(
            res,
            json!({
                "yql": concat!(
                    "select * from content where",
                    " {hnsw.exploreAdditionalHits:15, targetHits:10}nearestNeighbor(embedding, query_embedding)",
                    " and range(publication_date, 3661, Infinity)",
                    " and !(authors contains \"00000000-0000-4003-8000-000000000003\")",
                    " and (tags contains \"00000000-0000-4007-8000-000000000007\")",
                ),
                "input.query(query_embedding)": [2., 4., 6.],
                "ranking.profile": "ann",
                "hits": 10,
                "timeout": "60s",
            })
        );

        let res = yql_build_query(
            15,
            20,
            &[2., 4., 6.],
            &QueryPayload {
                publication_date: DateFilter::default(),
                authors: LabelFilter::default(),
                tags: LabelFilter::default(),
            },
            true,
            true,
        )
        .unwrap();

        assert_eq!(
            res,
            json!({
                "yql": concat!(
                    "select * from content where",
                    " {hnsw.exploreAdditionalHits:5, targetHits:15}nearestNeighbor(embedding, query_embedding)",
                ),
                "input.query(query_embedding)": [2., 4., 6.],
                "ranking.profile": "ann",
                "hits": 15,
                "timeout": "60s",
            })
        );

        let res = yql_build_query(
            10,
            20,
            &[2., 4., 6.],
            &QueryPayload {
                publication_date: DateFilter {
                    lower_bound: None,
                    upper_bound: Some(Utc.with_ymd_and_hms(1970, 1, 1, 1, 1, 1).unwrap()),
                },
                authors: LabelFilter {
                    include: Labels::default(),
                    exclude: Labels::default(),
                },
                tags: LabelFilter {
                    include: Labels::default(),
                    exclude: Labels::default(),
                },
            },
            false,
            true,
        )
        .unwrap();

        assert_eq!(
            res,
            json!({
                "yql": concat!(
                    "select id from content where",
                    " {hnsw.exploreAdditionalHits:10, targetHits:10}nearestNeighbor(embedding, query_embedding)",
                    " and range(publication_date, -Infinity, 3661)",
                ),
                "input.query(query_embedding)": [2., 4., 6.],
                "ranking.profile": "ann",
                "hits": 10,
                "timeout": "60s",
            })
        );

        let res = yql_build_query(
            10,
            20,
            &[2., 4., 6.],
            &QueryPayload {
                publication_date: DateFilter {
                    lower_bound: None,
                    upper_bound: Some(Utc.with_ymd_and_hms(1970, 1, 1, 1, 1, 1).unwrap()),
                },
                authors: LabelFilter {
                    include: Labels::default(),
                    exclude: Labels::default(),
                },
                tags: LabelFilter {
                    include: Labels::default(),
                    exclude: Labels::default(),
                },
            },
            false,
            false,
        )
        .unwrap();

        assert_eq!(
            res,
            json!({
                "yql": concat!(
                    "select id from content where",
                    " {hnsw.exploreAdditionalHits:10, targetHits:10}nearestNeighbor(embedding, query_embedding)",
                ),
                "input.query(query_embedding)": [2., 4., 6.],
                "ranking.profile": "ann",
                "hits": 10,
                "timeout": "60s",
            })
        );
    }
}
