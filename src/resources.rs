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
    cmp,
    fs::{self, File},
    io::{BufReader, BufWriter, Write},
    path::{Path, PathBuf},
    process::Command,
    str,
};

use anyhow::{anyhow, bail, Error};
use bincode::Options;
use chrono::Utc;
use serde::{de::DeserializeOwned, Serialize};
use serde_json::json;

#[derive(Serialize)]
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

    pub fn dataset_name(&self) -> Result<&str, Error> {
        Ok(self
            .vectors_file
            .file_stem()
            .and_then(|stem| stem.to_str())
            .ok_or_else(|| anyhow!("invalid vector file name"))?)
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

pub struct ResourceWriter {
    out_dir: PathBuf,
}

impl ResourceWriter {
    pub fn new(
        out_dir: impl Into<PathBuf>,
        scope: impl IntoIterator<Item = impl AsRef<str>>,
    ) -> Result<Self, Error> {
        let mut out_dir = out_dir.into();
        for segment in scope {
            out_dir.push(segment.as_ref());
        }
        let out_dir = create_next_dir(out_dir)?;
        let this = Self { out_dir };

        this.write_file(
            "open.json",
            &json!({
                "git": get_git_hash()?,
                "date": Utc::now().to_rfc3339(),
            }),
        )?;

        Ok(this)
    }

    pub fn sub_writer(&self, scope: impl AsRef<str>) -> Result<ResourceWriter, Error> {
        let out_dir = self.out_dir.join(scope.as_ref());
        fs::create_dir_all(&out_dir)?;
        Ok(Self { out_dir })
    }

    pub fn write_close_msg(self) -> Result<(), Error> {
        self.write_file(
            "close.json",
            &json!({
                "date": Utc::now().to_rfc3339(),
            }),
        )?;
        Ok(())
    }

    pub fn write_file(&self, name: impl AsRef<str>, data: &impl Serialize) -> Result<(), Error> {
        let out = File::options()
            .write(true)
            .create_new(true)
            .open(self.out_dir.join(name.as_ref()))?;
        let mut out = BufWriter::new(out);
        serde_json::to_writer(&mut out, data)?;
        out.write_all(&[b'\n'])?;
        out.flush()?;
        Ok(())
    }

    pub fn append_line_to_file(
        &self,
        name: impl AsRef<str>,
        data: &impl Serialize,
    ) -> Result<(), Error> {
        // Hint: A proper impl. might cache the fd we do not bother.
        // Hint: While this uses append it's not concurrent write safe.
        let out = File::options()
            .append(true)
            .open(self.out_dir.join(name.as_ref()))?;
        let mut out = BufWriter::new(out);
        serde_json::to_writer(&mut out, data)?;
        out.write_all(&[b'\n'])?;
        out.flush()?;
        Ok(())
    }
}

fn get_git_hash() -> Result<String, Error> {
    let out = Command::new("git").args(["rev-parse", "HEAD"]).output()?;
    if !out.status.success() {
        bail!("failed to run git rev-parse HEAD");
    }

    Ok(str::from_utf8(&out.stdout)?.trim().into())
}

fn create_next_dir(dir: impl AsRef<Path>) -> Result<PathBuf, Error> {
    let dir = dir.as_ref();
    fs::create_dir_all(&dir)?;

    let mut max: isize = -1;
    for entry in fs::read_dir(dir)? {
        let idx = entry?
            .file_name()
            .into_string()
            .map_err(|_| anyhow!("non utf-8 file name"))?
            .parse::<usize>()?
            .try_into()?;
        max = cmp::max(max, idx);
    }

    let out = dir.join(format!("{:0>4x}", max + 1));
    fs::create_dir_all(&out)?;
    Ok(out)
}
