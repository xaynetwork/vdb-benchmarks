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

use std::{
    fs,
    io::{BufReader, BufWriter, Write},
    path::{Path, PathBuf},
};

use anyhow::{anyhow, bail, Error};
use bincode::Options;
use serde::{de::DeserializeOwned, Serialize};

pub struct ResolvedPaths {
    pub vectors_file: PathBuf,
    pub document_payload_file: PathBuf,
    pub query_payload_file: PathBuf,
}

impl ResolvedPaths {
    pub fn resolve(vectors_file: impl Into<PathBuf>) -> Self {
        let vectors_file = vectors_file.into();
        let document_payload_file =
            with_different_file_ending(&vectors_file, "document.payload.bincode");
        let query_payload_file = with_different_file_ending(&vectors_file, "query.payload.bincode");
        Self {
            vectors_file,
            document_payload_file,
            query_payload_file,
        }
    }

    pub fn check_files_exists(&self) -> Result<(), Error> {
        if !self.vectors_file.is_file() {
            bail!("missing vectors file: {}", self.vectors_file.display());
        }
        if !self.document_payload_file.is_file() {
            bail!(
                "missing document payload file: {}",
                self.document_payload_file.display()
            );
        }
        if !self.query_payload_file.is_file() {
            bail!(
                "missing query payload file: {}",
                self.query_payload_file.display()
            );
        }
        Ok(())
    }

    pub fn check_only_vectors_file_exists(&self) -> Result<(), Error> {
        if !self.vectors_file.is_file() {
            bail!("missing vectors file: {}", self.vectors_file.display());
        }
        if self.document_payload_file.is_file() {
            bail!(
                "document payload file already exists: {}",
                self.document_payload_file.display()
            );
        }
        if self.query_payload_file.is_file() {
            bail!(
                "query payload file already exists: {}",
                self.query_payload_file.display()
            );
        }
        Ok(())
    }
}

fn with_different_file_ending(path: impl Into<PathBuf>, new_stem: impl AsRef<str>) -> PathBuf {
    let mut path = path.into();
    path.set_extension(new_stem.as_ref());
    path
}

pub fn dump_bincode<S>(path: &Path, data: &S) -> Result<(), Error>
where
    S: Serialize,
{
    let mut writer = BufWriter::new(fs::File::create(path)?);
    bincode::DefaultOptions::new().serialize_into(&mut writer, data)?;
    writer.flush()?;
    Ok(())
}

pub fn load_bincode<D>(path: &Path) -> Result<D, Error>
where
    D: DeserializeOwned,
{
    let reader = BufReader::new(fs::File::open(path)?);
    Ok(bincode::DefaultOptions::new().deserialize_from(reader)?)
}

pub fn load_vectors(path: &Path, dataset: &str) -> Result<Vec<Vec<f32>>, Error> {
    let file = hdf5::File::open(path)?;
    let dataset = file.dataset(dataset)?;
    // Warning: This can easily load 4+GiB of data
    //          To avoid having a single continuous 4GiB allocation and for convenience we
    //          read into a Vec<Vec<f32>> instead of an Array2<f32>.
    let vectors = (0..dataset.shape()[0])
        .map(|idx| match dataset.read_slice_1d(ndarray::s![idx, ..]) {
            Ok(array) => Ok(array.into_raw_vec()),
            Err(err) => Err(anyhow!("malformed vector dataset: {err}")),
        })
        .collect::<Result<Vec<_>, _>>()?;
    file.close()?;
    Ok(vectors)
}
