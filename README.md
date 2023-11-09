
# ANN q/s benchmark with filters and metadata.

This project provides some additional **rough** benchmarks
for aspects not benchmarked by [https://ann-benchmarks.com/index.html].

Mainly:

- assumes a fixed HNSW configuration with an acceptable "correctness"
- mainly measures queries per second
    - (rough: filters can still affect recall/precision)
- runs randomly sampled queries
  - with/without property filters
    - (rough: currently based on random but pre-computed/fixed data)
  - results contains/excludes additional non-filtered payload
- uses 3 nodes with 3 searchable shards and replication of at least 1
- uses similar interfaces as we would use in our product to access the vector database

## System Dependencies

- `cargo` and rust in general
- `just` as task runner (can be skipped by running all necessary commands by hand)
- `hdf5` version  `1.10.x`
    - (version 1.14.2 currently does not work and versions in between that and `1.10.x` should in general be avoided, hdf5 is not fully semver compliant)
    - to use a local build/download of hdf5
        - place dir structure (`/{include,lib}` etc.) in `.tools/libhdf5`
        - set env to absolute paths (assuming `$(pwd)`==project root)
          ```bash
          export HDF5_DIR="$(pwd)/.tools/libhdf5"
          export RUSTFLAGS="-C link-args=-Wl,-rpath,$HDF5_DIR/lib"
          ```
        - for vscode also set `rust-analyzer.cargo.extraEnv` to contain `HDF5_DIR`
        - or use nix-shell

    - you can use a local build and the `HDF5_DIR` environment variable (e.g. place it in `./.tools/libhdf5`)

## Data Generation

Run:

```bash
just generate-payload
# downloads ./resources/gitst-960-euclidean.hdf5 and generates ./resources/gist-960-euclidean.payload.hdf5
```

This will generate an additional output file which contains:

- a randomly sampled payload for each vector which based on `generation_settings.toml`
    - the payload has 3 fields `publication_date` (a datetime), `authors` (0+ authors), `tags` (0+ tags)
    - by default there are 20 authors to sample from and 200 tags
    - the strings of authors and tags are for simplicity just indices with a pure mapping to a uuid (pseudo v4)
- randomly sampled filters for each test vector
    - `publication_date` can be unfiltered, lower bound, upper bound and both bounds
    - `tags`/`authors` can have a number of required to be included and/or excluded `tags`/`authors`

## Volume Management

You can use `just volume {create|delete|recreate} {all|vespa|qdrant|elasticsearch}` to create storage volumes.
(Note that due to how vespa is currently setup you can't reuse that storage after stopping the server (this can be fixed), and with
elastic there might be issues, too (this is more troublesome to fix)).

## Service Management

You can use `just service {up|down} {qdrant|vespa|elasticsearch}` to start/stop services.

## Service Preparation

Use `just prepare {qdrant|vespa|elasticsearch}` to setup indices and ingest documents.

## Benchmark

You can use `just bench {qdrant|vespa|elasticsearch}` to runt he benchmarks.

## Report Handling

Reports will be generated in two places:

- `./reports/`
- `./target/criterion`

Continuous benchmark runs will will add to the reports, not override them.

You can use `just rm-reports` to delete this reports.

You can use `just cp-reports-for-commit` to copy them into `./committed_reports` creating
a structure like `./committed_reports/YYYY-MM-DD_HH:MM:SS.GITSHORTHASH/{additional_data,criterion}`.

For benchmarks run without filters we collect data for calculating recall and precision. Do
do so run `just recall <path>` which will recursive search the given `<path>` for files named
`recall_data.jsonl` and generate a `recall.json` alongside it as well as print the calculated
recall and precision. Normally you run it on `just recall ./reports` or `just recall ./committed_reports/..../additional_data`

Each bench has a id which looks like e.g. `qdrant/query_throughput/16:100_8.00:8.00-10:100pF-5:10` (this compact
form is necessary as there is a character limit for the id).

This id has following structure:

- `<id> := <provider> "/" <bench_group> "/" <ingestion-params> "_" <query-params>`
- `<ingestion-params> := <HNSW.M> ":" <HNSW.EF_CONSTRUCT>`
- `<query-params> := <limits> "-" <query> "-" <parallelism>`
- `<limits> := <cpu-limit> ":" <mem-limit>`
- `<query> := <k> ":" <ef/num_candidates> ":" <fetch-payload?true=P,false=p> ":" <use-filters?true=F,false=f>`
- `<parallelism> := <number-of-tasks> ":" <number-of-queries-per-task>`

Or all in one:

- `<provider> "/" <bench_group> "/" <HNSW.M> ":" <HNSW.EF_CONSTRUCT> "_" <cpu-limit> ":" <mem-limit> "-" <k> ":" <ef/num_candidates> ":" <fetch-payload?true=P,false=p> ":" <use-filters?true=F,false=f> "-" <number-of-tasks> ":" <number-of-queries-per-task>`

Be aware that the `num_candidates` parameter of elastic search is similar to `ef` but not quite them same, through we treat
them as the same as we have little other choice. Through if you notice some noticeable differences in recall/precision for benches with small `ef` value there is a good chance this is the reason for it.
