# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0-alpha.13](https://github.com/hickoryai/kory-cli/compare/gdk-v0.1.0-alpha.12...gdk-v0.1.0-alpha.13) - 2026-10-29

### Fixed

- *(local-inference)* keep nested execute fences inert ([#11117](https://github.com/hickoryai/kory-cli/pull/11117))
- unify context limit resolution behind provider API ([#11213](https://github.com/hickoryai/kory-cli/pull/11213))
- *(security)* preserve local inference message text ([#11452](https://github.com/hickoryai/kory-cli/pull/11452))
- *(local-inference)* discover GGUF repos with non-standard filenames ([#11005](https://github.com/hickoryai/kory-cli/pull/11005))
- *(local-inference)* keep XML snippets inside JSON as data ([#10606](https://github.com/hickoryai/kory-cli/pull/10606))
- *(local-inference)* preserve featured model size on delete and backfill missing sizes ([#10422](https://github.com/hickoryai/kory-cli/pull/10422))
- *(config)* require absolute goose path roots ([#10454](https://github.com/hickoryai/kory-cli/pull/10454))
- avoid local inference Metal teardown crash ([#10508](https://github.com/hickoryai/kory-cli/pull/10508))
- guard llama.cpp backend init against SIGILL on x86_64 CPUs without FMA/AVX2 ([#10105](https://github.com/hickoryai/kory-cli/pull/10105))

### Other

- Replace PCTX code mode with a Boa JavaScript runtime ([#12952](https://github.com/hickoryai/kory-cli/pull/12952))
- *(GDK)* release v0.1.0-alpha.12 ([#12561](https://github.com/hickoryai/kory-cli/pull/12561))
- *(GDK)* release v0.1.0-alpha.11 ([#12495](https://github.com/hickoryai/kory-cli/pull/12495))
- *(GDK)* release v0.1.0-alpha.10 ([#12492](https://github.com/hickoryai/kory-cli/pull/12492))
- *(GDK)* release v0.1.0-alpha.9 ([#11866](https://github.com/hickoryai/kory-cli/pull/11866))
- goose-sdk version bump alpha 8 ([#11815](https://github.com/hickoryai/kory-cli/pull/11815))
- *(local-inference)* remove managed model registry ([#11789](https://github.com/hickoryai/kory-cli/pull/11789))
- publish GDK packages from version tags ([#11595](https://github.com/hickoryai/kory-cli/pull/11595))
- add SDK API reference for Rust, Python, and Kotlin ([#11251](https://github.com/hickoryai/kory-cli/pull/11251))
- *(deps)* bump safemlx-lm from 0.1.5 to 0.4.1 ([#11464](https://github.com/hickoryai/kory-cli/pull/11464))
- *(goose-local-inference)* move mlx deps under macos ([#11328](https://github.com/hickoryai/kory-cli/pull/11328))
- create the goose-agent crate with the unrolled agent loop state machine ([#11216](https://github.com/hickoryai/kory-cli/pull/11216))
- enhance the uniffi API layer ([#10427](https://github.com/hickoryai/kory-cli/pull/10427))
- remove unused utoipa ToSchema derives ([#10505](https://github.com/hickoryai/kory-cli/pull/10505))
- Update Rust toolchain and raise recursion limit ([#10303](https://github.com/hickoryai/kory-cli/pull/10303))
- Switch the local inference provider MLX backend to use the safemlx crate ([#10304](https://github.com/hickoryai/kory-cli/pull/10304))
- add provider bindings MVP to goose-sdk, and add python wheel publishing ([#10208](https://github.com/hickoryai/kory-cli/pull/10208))
- Refactor local inference provider crates ([#10169](https://github.com/hickoryai/kory-cli/pull/10169))

## [0.1.0-alpha.12](https://github.com/aaif-goose/goose/compare/gdk-v0.1.0-alpha.11...gdk-v0.1.0-alpha.12) - 2026-10-13

### Other

- update Cargo.toml dependencies

## [0.1.0-alpha.11](https://github.com/aaif-goose/goose/compare/gdk-v0.1.0-alpha.10...gdk-v0.1.0-alpha.11) - 2026-09-28

### Other

- update Cargo.toml dependencies

## [0.1.0-alpha.9](https://github.com/aaif-goose/goose/compare/gdk-v0.1.0-alpha.8...gdk-v0.1.0-alpha.9) - 2026-09-08

### Other

- update Cargo.toml dependencies
