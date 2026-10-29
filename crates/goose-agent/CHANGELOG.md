# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0-alpha.13](https://github.com/hickoryai/kory-cli/compare/gdk-v0.1.0-alpha.12...gdk-v0.1.0-alpha.13) - 2026-10-29

### Added

- surface provider x-request-id on responses and errors ([#12587](https://github.com/hickoryai/kory-cli/pull/12587))
- *(goose-agent)* add tool calling ([#11627](https://github.com/hickoryai/kory-cli/pull/11627))

### Fixed

- *(agent)* never resend a prompt to a provider that keeps its own conversation
- *(agents)* keep streamed thinking ahead of text and tool calls ([#11837](https://github.com/hickoryai/kory-cli/pull/11837))
- *(anthropic)* preserved-thinking compliance for the provider layer ([#11836](https://github.com/hickoryai/kory-cli/pull/11836))

### Other

- Merge official Goose main into the router ACP branch
- Keep Goose tool context out of native agent prompts
- *(GDK)* release v0.1.0-alpha.12 ([#12561](https://github.com/hickoryai/kory-cli/pull/12561))
- not infer or steer while a tool approval is pending ([#12808](https://github.com/hickoryai/kory-cli/pull/12808))
- changed yield_to_customer to next enum ([#12789](https://github.com/hickoryai/kory-cli/pull/12789))
- rename to finalize_cancellation and make Emitter sync ([#12786](https://github.com/hickoryai/kory-cli/pull/12786))
- agent loop: make Stop end the run promptly and save what it produced ([#12765](https://github.com/hickoryai/kory-cli/pull/12765))
- Delete the legacy agent loop ([#12760](https://github.com/hickoryai/kory-cli/pull/12760))
- Resolve extensions into a per-inference lease ([#12161](https://github.com/hickoryai/kory-cli/pull/12161))
- Run the GDK agent loop on wasm32 ([#12569](https://github.com/hickoryai/kory-cli/pull/12569))
- *(GDK)* release v0.1.0-alpha.11 ([#12495](https://github.com/hickoryai/kory-cli/pull/12495))
- don't emit warning when ending on a successful tool call ([#12468](https://github.com/hickoryai/kory-cli/pull/12468))
- *(GDK)* release v0.1.0-alpha.10 ([#12492](https://github.com/hickoryai/kory-cli/pull/12492))
- *(GDK)* release v0.1.0-alpha.9 ([#11866](https://github.com/hickoryai/kory-cli/pull/11866))
- goose-sdk version bump alpha 8 ([#11815](https://github.com/hickoryai/kory-cli/pull/11815))
- publish GDK packages from version tags ([#11595](https://github.com/hickoryai/kory-cli/pull/11595))
- Clean up obsolete extension paths before redesigning the extension manager ([#11645](https://github.com/hickoryai/kory-cli/pull/11645))
- move inference operation into goose-agent ([#11294](https://github.com/hickoryai/kory-cli/pull/11294))
- add SDK API reference for Rust, Python, and Kotlin ([#11251](https://github.com/hickoryai/kory-cli/pull/11251))
- create the goose-agent crate with the unrolled agent loop state machine ([#11216](https://github.com/hickoryai/kory-cli/pull/11216))

## [0.1.0-alpha.12](https://github.com/aaif-goose/goose/compare/gdk-v0.1.0-alpha.11...gdk-v0.1.0-alpha.12) - 2026-10-13

### Added

- surface provider x-request-id on responses and errors ([#12587](https://github.com/aaif-goose/goose/pull/12587))

### Other

- not infer or steer while a tool approval is pending ([#12808](https://github.com/aaif-goose/goose/pull/12808))
- changed yield_to_customer to next enum ([#12789](https://github.com/aaif-goose/goose/pull/12789))
- rename to finalize_cancellation and make Emitter sync ([#12786](https://github.com/aaif-goose/goose/pull/12786))
- agent loop: make Stop end the run promptly and save what it produced ([#12765](https://github.com/aaif-goose/goose/pull/12765))
- Delete the legacy agent loop ([#12760](https://github.com/aaif-goose/goose/pull/12760))
- Resolve extensions into a per-inference lease ([#12161](https://github.com/aaif-goose/goose/pull/12161))
- Run the GDK agent loop on wasm32 ([#12569](https://github.com/aaif-goose/goose/pull/12569))

## [0.1.0-alpha.11](https://github.com/aaif-goose/goose/compare/gdk-v0.1.0-alpha.10...gdk-v0.1.0-alpha.11) - 2026-09-28

### Other

- don't emit warning when ending on a successful tool call ([#12468](https://github.com/aaif-goose/goose/pull/12468))

## [0.1.0-alpha.10](https://github.com/aaif-goose/goose/compare/gdk-v0.1.0-alpha.9...gdk-v0.1.0-alpha.10) - 2026-09-24

### Fixed

- *(agents)* keep streamed thinking ahead of text and tool calls ([#11837](https://github.com/aaif-goose/goose/pull/11837))
- *(anthropic)* preserved-thinking compliance for the provider layer ([#11836](https://github.com/aaif-goose/goose/pull/11836))

## [0.1.0-alpha.9](https://github.com/aaif-goose/goose/compare/gdk-v0.1.0-alpha.8...gdk-v0.1.0-alpha.9) - 2026-09-08

### Other

- update Cargo.toml dependencies
