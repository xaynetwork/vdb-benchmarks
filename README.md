
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
- uses 3 storage nodes with a replication of 2
- uses the same interfaces we would use in our product

## System Dependencies

- `cargo` and rust in general
- `just` as task runner (can be skipped by running all necessary commands by hand)
- `hdf5` version  `1.10.10`
    - (version 1.14.2 currently does not work and versions in between that and `1.10.10` should in general be avoided, hdf5 is not fully semver compliant)
    - to use a local build/download of hdf5
        - place dir structure (`/{include,lib}` etc.) in `.tools/libhdf5`
        - set env to absolute paths (assuming `$(pwd)`==project root)
          ```bash
          export HDF5_DIR="$(pwd)/.tools/libhdf5"
          export RUSTFLAGS="-C link-args=-Wl,-rpath,$HDF5_DIR/lib"
          ```
        - for vscode also set `rust-analyzer.cargo.extraEnv` to contain `HDF5_DIR`

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

