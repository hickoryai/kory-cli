# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0-alpha.13](https://github.com/hickoryai/kory-cli/compare/gdk-v0.1.0-alpha.12...gdk-v0.1.0-alpha.13) - 2026-10-29

### Added

- surface provider x-request-id on responses and errors ([#12587](https://github.com/hickoryai/kory-cli/pull/12587))
- Use live model metadata for OpenAI reasoning and prefer the Responses API ([#12568](https://github.com/hickoryai/kory-cli/pull/12568))
- *(gdk)* add model discovery to the Provider API ([#12497](https://github.com/hickoryai/kory-cli/pull/12497))
- *(sdk)* allow OpenAI provider to accept a custom base URL ([#11868](https://github.com/hickoryai/kory-cli/pull/11868)) ([#11967](https://github.com/hickoryai/kory-cli/pull/11967))
- *(gdk)* let Kotlin callers configure the Databricks AI Gateway path ([#11631](https://github.com/hickoryai/kory-cli/pull/11631))
- *(gdk)* add first-class document content support ([#11629](https://github.com/hickoryai/kory-cli/pull/11629))
- *(gdk)* expose Anthropic response metadata for observability ([#11630](https://github.com/hickoryai/kory-cli/pull/11630))
- *(gdk)* add structured request and response observability hooks ([#11632](https://github.com/hickoryai/kory-cli/pull/11632))
- *(gdk)* preserve thinking and redacted-thinking blocks across turns ([#11636](https://github.com/hickoryai/kory-cli/pull/11636))
- *(gdk)* expose cached input-token usage metrics ([#11628](https://github.com/hickoryai/kory-cli/pull/11628))
- compaction in the GDK ([#11042](https://github.com/hickoryai/kory-cli/pull/11042))
- *(sdk)* minimal uniffi setup for cross language sdk ([#9593](https://github.com/hickoryai/kory-cli/pull/9593))
- projects as backend sources with system prompt injection ([#8739](https://github.com/hickoryai/kory-cli/pull/8739))
- *(acp)* expose built-in skills through sources list acp calls ([#9045](https://github.com/hickoryai/kory-cli/pull/9045))
- move goose2 provider catalog behind ACP layer ([#9030](https://github.com/hickoryai/kory-cli/pull/9030))
- *(acp)* replace raw config and secret methods ([#9000](https://github.com/hickoryai/kory-cli/pull/9000))
- goose2 add support for custom providers in ui & acp ([#8924](https://github.com/hickoryai/kory-cli/pull/8924))
- migrate session metadata storage from frontend overlay to backend ([#8769](https://github.com/hickoryai/kory-cli/pull/8769))
- associate threads with projects ([#8745](https://github.com/hickoryai/kory-cli/pull/8745))
- *(goose2)* voice dictation via direct-ACP pattern ([#8609](https://github.com/hickoryai/kory-cli/pull/8609))
- *(tui)* add extension management screen ([#8536](https://github.com/hickoryai/kory-cli/pull/8536))
- provider & model config ([#8515](https://github.com/hickoryai/kory-cli/pull/8515))
- onboarding UX for the TUI ([#8513](https://github.com/hickoryai/kory-cli/pull/8513))
- *(acp)* introduce threads ([#8344](https://github.com/hickoryai/kory-cli/pull/8344))
- *(acp)* add reusable ACP provider controls ([#8314](https://github.com/hickoryai/kory-cli/pull/8314))

### Fixed

- *(gdk)* resolve vision support from the canonical catalog so tool result images reach the model ([#12440](https://github.com/hickoryai/kory-cli/pull/12440))
- *(anthropic)* preserved-thinking compliance for the provider layer ([#11836](https://github.com/hickoryai/kory-cli/pull/11836))
- fix gdk kotlin example ([#11825](https://github.com/hickoryai/kory-cli/pull/11825))
- *(gdk)* repair uniffi build broken by semantic merge conflict ([#11721](https://github.com/hickoryai/kory-cli/pull/11721))
- *(gdk)* preserve tool-call indices in streaming responses ([#11637](https://github.com/hickoryai/kory-cli/pull/11637))
- unify context limit resolution behind provider API ([#11213](https://github.com/hickoryai/kory-cli/pull/11213))
- *(providers)* update DeepSeek model names to v4 API ([#10729](https://github.com/hickoryai/kory-cli/pull/10729))

### Other

- ship the CLI binary as kory
- *(GDK)* release v0.1.0-alpha.12 ([#12561](https://github.com/hickoryai/kory-cli/pull/12561))
- *(GDK)* release v0.1.0-alpha.11 ([#12495](https://github.com/hickoryai/kory-cli/pull/12495))
- *(GDK)* release v0.1.0-alpha.10 ([#12492](https://github.com/hickoryai/kory-cli/pull/12492))
- Add a decisions provider crate, with impls for openrouter and jev ([#12418](https://github.com/hickoryai/kory-cli/pull/12418))
- *(GDK)* release v0.1.0-alpha.9 ([#11866](https://github.com/hickoryai/kory-cli/pull/11866))
- use release-plz to prepare GDK releases ([#11820](https://github.com/hickoryai/kory-cli/pull/11820))
- goose-sdk version bump alpha 8 ([#11815](https://github.com/hickoryai/kory-cli/pull/11815))
- publish GDK packages from version tags ([#11595](https://github.com/hickoryai/kory-cli/pull/11595))
- add SDK API reference for Rust, Python, and Kotlin ([#11251](https://github.com/hickoryai/kory-cli/pull/11251))
- pin V1 SDK and narrow acp crate features ([#11289](https://github.com/hickoryai/kory-cli/pull/11289))
- create the goose-agent crate with the unrolled agent loop state machine ([#11216](https://github.com/hickoryai/kory-cli/pull/11216))
- Use rmcp ContentBlock in goose-sdk UniFFI ([#11210](https://github.com/hickoryai/kory-cli/pull/11210))
- enhance the uniffi API layer ([#10427](https://github.com/hickoryai/kory-cli/pull/10427))
- add build and maven publish for kotlin ffi ([#10375](https://github.com/hickoryai/kory-cli/pull/10375))
- *(deps)* bump uniffi from 0.31.1 to 0.32.0 ([#10247](https://github.com/hickoryai/kory-cli/pull/10247))
- add provider bindings MVP to goose-sdk, and add python wheel publishing ([#10208](https://github.com/hickoryai/kory-cli/pull/10208))
- *(acp)* upgrade SDK and use new HTTP/WS crate ([#10082](https://github.com/hickoryai/kory-cli/pull/10082))
- *(deps)* bump uniffi from 0.29.5 to 0.31.1 ([#9628](https://github.com/hickoryai/kory-cli/pull/9628))
- Lifei/acp session setup refactor ([#9488](https://github.com/hickoryai/kory-cli/pull/9488))
- Expose raw provider supported models over ACP ([#9475](https://github.com/hickoryai/kory-cli/pull/9475))
- Add ACP session system prompt setter ([#9478](https://github.com/hickoryai/kory-cli/pull/9478))
- protocol cleanup ([#9147](https://github.com/hickoryai/kory-cli/pull/9147))
- Preserve thinking content for providers that require it ([#8857](https://github.com/hickoryai/kory-cli/pull/8857))
- Agents crud ([#9084](https://github.com/hickoryai/kory-cli/pull/9084))
- switch to official new rust-sdk for ACP ([#9062](https://github.com/hickoryai/kory-cli/pull/9062))
- add provider-first onboarding ([#9039](https://github.com/hickoryai/kory-cli/pull/9039))
- render mcp apps inline in goose2 ([#8877](https://github.com/hickoryai/kory-cli/pull/8877))
- update goose2 credential management behind provider-scoped ACP/core API ([#8887](https://github.com/hickoryai/kory-cli/pull/8887))
- Dedupe and organize skills/sources ([#8731](https://github.com/hickoryai/kory-cli/pull/8731))
- commands to acp+ migration: extensions management ([#8733](https://github.com/hickoryai/kory-cli/pull/8733))
- consolidate provider ACP methods onto inventory ([#8710](https://github.com/hickoryai/kory-cli/pull/8710))
- declare and enforce MSRV of 1.91.1 ([#8670](https://github.com/hickoryai/kory-cli/pull/8670))
- Manage skills as sources over ACP ([#8675](https://github.com/hickoryai/kory-cli/pull/8675))
- overhaul provider inventory and agent/model selection ([#8652](https://github.com/hickoryai/kory-cli/pull/8652))
- gate perf logs and dedup build_config_update on first message ([#8627](https://github.com/hickoryai/kory-cli/pull/8627))
- rust acp client for extension methods ([#8227](https://github.com/hickoryai/kory-cli/pull/8227))

## [0.1.0-alpha.12](https://github.com/aaif-goose/goose/compare/gdk-v0.1.0-alpha.11...gdk-v0.1.0-alpha.12) - 2026-10-13

### Added

- surface provider x-request-id on responses and errors ([#12587](https://github.com/aaif-goose/goose/pull/12587))
- Use live model metadata for OpenAI reasoning and prefer the Responses API ([#12568](https://github.com/aaif-goose/goose/pull/12568))

## [0.1.0-alpha.11](https://github.com/aaif-goose/goose/compare/gdk-v0.1.0-alpha.10...gdk-v0.1.0-alpha.11) - 2026-09-28

### Added

- *(gdk)* add model discovery to the Provider API ([#12497](https://github.com/aaif-goose/goose/pull/12497))

## [0.1.0-alpha.10](https://github.com/aaif-goose/goose/compare/gdk-v0.1.0-alpha.9...gdk-v0.1.0-alpha.10) - 2026-09-24

### Added

- *(sdk)* allow OpenAI provider to accept a custom base URL ([#11868](https://github.com/aaif-goose/goose/pull/11868)) ([#11967](https://github.com/aaif-goose/goose/pull/11967))

### Fixed

- *(gdk)* resolve vision support from the canonical catalog so tool result images reach the model ([#12440](https://github.com/aaif-goose/goose/pull/12440))
- *(anthropic)* preserved-thinking compliance for the provider layer ([#11836](https://github.com/aaif-goose/goose/pull/11836))

### Other

- Add a decisions provider crate, with impls for openrouter and jev ([#12418](https://github.com/aaif-goose/goose/pull/12418))

## [0.1.0-alpha.9](https://github.com/aaif-goose/goose/compare/gdk-v0.1.0-alpha.8...gdk-v0.1.0-alpha.9) - 2026-09-08

### Fixed

- fix gdk kotlin example ([#11825](https://github.com/aaif-goose/goose/pull/11825))

### Other

- use release-plz to prepare GDK releases ([#11820](https://github.com/aaif-goose/goose/pull/11820))
