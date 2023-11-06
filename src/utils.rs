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

use std::future::Future;

use anyhow::{anyhow, Error};
use reqwest::Response;

pub(crate) async fn body_to_error(response: Response) -> Error {
    match response.bytes().await {
        Ok(bytes) => {
            let text = String::from_utf8_lossy(&bytes);
            anyhow!("request failed: {text}")
        }
        Err(error) => error.into(),
    }
}

pub(crate) async fn await_and_check_request(
    fut: impl Future<Output = Result<Response, reqwest::Error>>,
) -> Result<Response, Error> {
    let response = fut.await?;
    if response.status().is_success() {
        Ok(response)
    } else {
        Err(body_to_error(response).await)
    }
}
