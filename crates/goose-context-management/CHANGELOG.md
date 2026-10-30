# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0-alpha.13](https://github.com/hickoryai/kory-cli/compare/gdk-v0.1.0-alpha.12...gdk-v0.1.0-alpha.13) - 2026-10-30

### Added

- a lean ACP-only Goose binary ([#11961](https://github.com/hickoryai/kory-cli/pull/11961))
- *(gdk)* add first-class document content support ([#11629](https://github.com/hickoryai/kory-cli/pull/11629))
- compaction in the GDK ([#11042](https://github.com/hickoryai/kory-cli/pull/11042))

### Fixed

- fix context management dependency ([#11768](https://github.com/hickoryai/kory-cli/pull/11768))
- unify context limit resolution behind provider API ([#11213](https://github.com/hickoryai/kory-cli/pull/11213))
- honest compaction failure message and fast-fail when no tool responses exist ([#10500](https://github.com/hickoryai/kory-cli/pull/10500))
- *(agents)* fail fast when a recipe's structured response can't reach an ACP-bridged provider ([#11307](https://github.com/hickoryai/kory-cli/pull/11307))

### Other

- *(GDK)* release v0.1.0-alpha.12 ([#12561](https://github.com/hickoryai/kory-cli/pull/12561))
- *(GDK)* release v0.1.0-alpha.11 ([#12495](https://github.com/hickoryai/kory-cli/pull/12495))
- *(GDK)* release v0.1.0-alpha.10 ([#12492](https://github.com/hickoryai/kory-cli/pull/12492))
- *(GDK)* release v0.1.0-alpha.9 ([#11866](https://github.com/hickoryai/kory-cli/pull/11866))
- goose-sdk version bump alpha 8 ([#11815](https://github.com/hickoryai/kory-cli/pull/11815))
- publish GDK packages from version tags ([#11595](https://github.com/hickoryai/kory-cli/pull/11595))
- Clean up obsolete extension paths before redesigning the extension manager ([#11645](https://github.com/hickoryai/kory-cli/pull/11645))
- add SDK API reference for Rust, Python, and Kotlin ([#11251](https://github.com/hickoryai/kory-cli/pull/11251))
- create the goose-agent crate with the unrolled agent loop state machine ([#11216](https://github.com/hickoryai/kory-cli/pull/11216))

## [0.1.0-alpha.12](https://github.com/aaif-goose/goose/compare/gdk-v0.1.0-alpha.11...gdk-v0.1.0-alpha.12) - 2026-10-13

### Other

- update Cargo.toml dependencies

## [0.1.0-alpha.11](https://github.com/aaif-goose/goose/compare/gdk-v0.1.0-alpha.10...gdk-v0.1.0-alpha.11) - 2026-09-28

### Other

- update Cargo.toml dependencies

## [0.1.0-alpha.10](https://github.com/aaif-goose/goose/compare/gdk-v0.1.0-alpha.9...gdk-v0.1.0-alpha.10) - 2026-09-24

### Added

- a lean ACP-only Goose binary ([#11961](https://github.com/aaif-goose/goose/pull/11961))

### Fixed

- fix context management dependency ([#11768](https://github.com/aaif-goose/goose/pull/11768))

## [0.1.0-alpha.9](https://github.com/aaif-goose/goose/compare/gdk-v0.1.0-alpha.8...gdk-v0.1.0-alpha.9) - 2026-09-08

### Other

- update Cargo.toml dependencies
