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

use std::process::Stdio;

use anyhow::{anyhow, bail, Context, Error};
use regex::Regex;
use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
    runtime::Runtime,
    select,
    sync::oneshot::{self, Sender},
    task::JoinHandle,
};

use crate::math::WelfordOnlineAlgorithm;

pub struct DockerStatScanner {
    handle: JoinHandle<Result<DockerStats, Error>>,
    sender: Sender<Stop>,
}

#[derive(Default, Serialize)]
pub struct DockerStats {
    memory: Stat,
    cpu: Stat,
    block_io: Stat,
    net_io: Stat,
}

impl DockerStats {
    fn update(&mut self, point: StatPoint) {
        self.memory.update(point.memory);
        self.cpu.update(point.cpu);
        self.block_io.update(point.block_io);
        self.net_io.update(point.net_io);
    }
}

#[derive(Default, Serialize)]
pub struct Stat {
    max: f64,
    min: f64,
    dist: WelfordOnlineAlgorithm,
}

impl Stat {
    fn update(&mut self, value: f64) {
        self.max = self.max.max(value);
        self.min = self.min.min(value);
        self.dist.update(value);
    }
}

struct StatPoint {
    memory: f64,
    cpu: f64,
    block_io: f64,
    net_io: f64,
}

struct Stop;

impl DockerStatScanner {
    pub fn start(rt: &Runtime, provider: &str) -> Result<DockerStatScanner, Error> {
        let line_parser = LineParser::new(provider)?;
        let (sender, mut receiver) = oneshot::channel();
        let handle = rt.spawn(async move {
            //Note: docker stats probes only every few seconds and has a initial delay
            //      this means it has a rather bad granularity and might not work at all
            //      for very short running jobs.
            let mut handle = Command::new("docker")
                .args(["stats", "--format", "{{ json . }}"])
                .stdout(Stdio::piped())
                .kill_on_drop(true)
                .spawn()?;

            let mut stats = DockerStats::default();
            let mut lines = BufReader::new(handle.stdout.take().unwrap()).lines();
            loop {
                select! {
                    line = lines.next_line() => {
                        let Some(line) = line? else {
                            break;
                        };

                        if let Some(point) = line_parser.parse(&line)
                            .with_context(|| format!("parse docker line: {line:?}"))? {
                            stats.update(point);
                        }
                    },
                    stop = &mut receiver => {
                        stop?;
                        break;
                    }
                }
            }
            drop(lines);
            handle.start_kill()?;
            Ok(stats)
        });

        Ok(Self { handle, sender })
    }

    pub async fn stop(self) -> Result<DockerStats, Error> {
        self.sender.send(Stop).ok();
        Ok(self.handle.await??)
    }
}

struct LineParser {
    regex: Regex,
    name_prefix: String,
}

#[allow(non_upper_case_globals)]
const GiB: f64 = 0x40000000 as _;
#[allow(non_upper_case_globals)]
const GB: f64 = 1_000_000_000.;

impl LineParser {
    fn new(name_prefix: impl Into<String>) -> Result<Self, Error> {
        Ok(Self {
            regex: Regex::new(r#"\u{1b}\[[0-9;]*[a-zA-Z]"#)?,
            name_prefix: name_prefix.into(),
        })
    }

    fn parse(&self, line: &str) -> Result<Option<StatPoint>, Error> {
        let line = self.regex.replace_all(line, "");
        if line.is_empty() {
            return Ok(None);
        }

        let StatPointJson {
            name,
            memory,
            cpu,
            block_io,
            net_io,
        } = serde_json::from_str(line.trim())?;

        if !name.starts_with(&self.name_prefix) {
            return Ok(None);
        }

        Ok(Some(StatPoint {
            memory: parse_mem_current_max(&memory, GiB as _)?,
            cpu: parse_percentage(&cpu)?,
            block_io: parse_mem_current_max(&block_io, GB as _)?,
            net_io: parse_mem_current_max(&net_io, GB as _)?,
        }))
    }
}

fn parse_mem_current_max(input: &str, divide_by: f64) -> Result<f64, Error> {
    let s = input
        .split('/')
        .next()
        .ok_or_else(|| anyhow!("unexpected docker stats format: {input}"))?
        .trim();
    let s = s
        .strip_suffix("B")
        .ok_or_else(|| anyhow!("unexpected docker stats format: {input}"))?;
    let (s, base, pow_multiplier) = s.strip_suffix('i').map_or((s, 10, 3), |s| (s, 2, 10));
    let (s, power_level) = s
        .strip_suffix('K')
        .map(|s| (s, 1))
        .or_else(|| s.strip_suffix('M').map(|s| (s, 2)))
        .or_else(|| s.strip_suffix('G').map(|s| (s, 3)))
        .unwrap_or((s, 0));

    if power_level == 0u32 && base == 2u64 {
        bail!("iB is not a valid unit: {input}");
    }
    let number: f64 = s.parse()?;

    Ok(number * base.pow(power_level * pow_multiplier) as f64 / divide_by)
}

fn parse_percentage(value: &str) -> Result<f64, Error> {
    let value = value.trim();
    if let Some(number) = value.strip_suffix('%') {
        Ok(number.parse()?)
    } else {
        bail!("malformed percentage value: {value:?}");
    }
}

#[derive(Deserialize)]
struct StatPointJson {
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "MemUsage")]
    memory: String,
    #[serde(rename = "CPUPerc")]
    cpu: String,
    #[serde(rename = "BlockIO")]
    block_io: String,
    #[serde(rename = "NetIO")]
    net_io: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parsing_dual_mem_stats() {
        assert_eq!(parse_mem_current_max("10GiB / 20GiB", GiB).unwrap(), 10.);
        assert_eq!(parse_mem_current_max("1024MiB / 20GiB", GiB).unwrap(), 1.);
        assert_eq!(parse_mem_current_max("2500000KB / 20GiB", GB).unwrap(), 2.5);
        assert_eq!(
            parse_mem_current_max("4250000000B / 20GiB", GB).unwrap(),
            4.25
        );
    }
}
