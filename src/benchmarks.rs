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

pub mod query_throughput;

use std::fmt::{self, Display};

use anyhow::Error;
use async_trait::async_trait;
use uuid::Uuid;

use crate::distribution::QueryPayload;

#[derive(Clone, Copy)]
pub struct QueryParameters {
    pub k: usize,
    pub ef: usize,
    pub fetch_payload: bool,
    pub number_of_tasks: usize,
    pub queries_per_task: usize,
    pub use_filters: bool,
}

impl Display for QueryParameters {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            k,
            ef,
            fetch_payload,
            number_of_tasks,
            queries_per_task,
            use_filters,
        } = self;
        let fetch_payload = if *fetch_payload { "P" } else { "p" };
        let use_filters = if *use_filters { "F" } else { "f" };
        write!(
            f,
            "{k}-{ef}{fetch_payload}{use_filters}-{number_of_tasks}-{queries_per_task}"
        )
    }
}

#[derive(Clone, Copy)]
pub struct IngestionParameters {
    pub m: usize,
    pub ef_construct: usize,
}

impl Display for IngestionParameters {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self { m, ef_construct } = self;
        write!(f, "{m}-{ef_construct}")
    }
}

#[async_trait]
pub trait QueryVectorDatabase: Send + Sync + 'static {
    fn name(&self) -> &str;
    async fn query(
        &self,
        k: usize,
        ef: usize,
        vector: &[f32],
        payload: &QueryPayload,
        return_payload: bool,
        use_filters: bool,
    ) -> Result<Vec<Uuid>, Error>;
}
