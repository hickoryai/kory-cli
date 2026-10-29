# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0-alpha.13](https://github.com/hickoryai/kory-cli/compare/gdk-v0.1.0-alpha.12...gdk-v0.1.0-alpha.13) - 2026-10-29

### Added

- add Live voice conversations to the desktop app ([#12093](https://github.com/hickoryai/kory-cli/pull/12093))
- *(acp)* scope Toolshim to custom providers ([#11414](https://github.com/hickoryai/kory-cli/pull/11414))
- *(mcp)* support pre-registered OAuth clients for streamable_http extensions ([#11182](https://github.com/hickoryai/kory-cli/pull/11182))
- *(acp)* add _goose/unstable/session/extensions/apply method ([#11081](https://github.com/hickoryai/kory-cli/pull/11081))
- *(desktop)* add markdown format option to session export ([#10703](https://github.com/hickoryai/kory-cli/pull/10703))
- *(acp)* add delete support for custom apps from Apps UI ([#10127](https://github.com/hickoryai/kory-cli/pull/10127))
- *(desktop)* per-message usage stats UI (tokens, cost, TTFT, tok/s) ([#10210](https://github.com/hickoryai/kory-cli/pull/10210))
- feat (ui): Migrate dictation local model manager to ACP ([#10131](https://github.com/hickoryai/kory-cli/pull/10131))
- feat (acp): exposed available tools in acp schema ([#10097](https://github.com/hickoryai/kory-cli/pull/10097))
- *(acp+)* config in desktop on ACP+ ([#10057](https://github.com/hickoryai/kory-cli/pull/10057))
- *(acp+)* models/providers in desktop on ACP+ ([#9987](https://github.com/hickoryai/kory-cli/pull/9987))
- *(acp)* migrate upsertPermissions to setToolPermissions ACP method ([#9999](https://github.com/hickoryai/kory-cli/pull/9999))
- feat (acp): custom methods for managing scheduler ([#9972](https://github.com/hickoryai/kory-cli/pull/9972))
- created acp method for managing recipes and UI changes ([#9951](https://github.com/hickoryai/kory-cli/pull/9951))
- feat (acp): used typed request and response for add, get session extensions ([#9890](https://github.com/hickoryai/kory-cli/pull/9890))
- feat (ui): Add durable ACP chat session state for desktop UI ([#9852](https://github.com/hickoryai/kory-cli/pull/9852))
- implement acp method for elicitation and elicitation improvement ([#9797](https://github.com/hickoryai/kory-cli/pull/9797))
- custom acp method to get session info ([#9729](https://github.com/hickoryai/kory-cli/pull/9729))
- steering messages with ACP ([#9560](https://github.com/hickoryai/kory-cli/pull/9560))
- acp methods for config extensions  ([#9581](https://github.com/hickoryai/kory-cli/pull/9581))
- *(sdk)* minimal uniffi setup for cross language sdk ([#9593](https://github.com/hickoryai/kory-cli/pull/9593))

### Fixed

- *(acp)* bind MCP app tools to extension owners ([#11416](https://github.com/hickoryai/kory-cli/pull/11416))
- *(security)* preserve session extension identities ([#11420](https://github.com/hickoryai/kory-cli/pull/11420))
- *(providers)* inform user of clipboard copy and remove copilot auth retry on timeout ([#11160](https://github.com/hickoryai/kory-cli/pull/11160))
- bind deeplink recipe parameters to startup session ([#11123](https://github.com/hickoryai/kory-cli/pull/11123))

### Other

- *(GDK)* release v0.1.0-alpha.12 ([#12561](https://github.com/hickoryai/kory-cli/pull/12561))
- Remove Nostr session sharing from goose core ([#12453](https://github.com/hickoryai/kory-cli/pull/12453))
- *(GDK)* release v0.1.0-alpha.11 ([#12495](https://github.com/hickoryai/kory-cli/pull/12495))
- *(GDK)* release v0.1.0-alpha.10 ([#12492](https://github.com/hickoryai/kory-cli/pull/12492))
- *(GDK)* release v0.1.0-alpha.9 ([#11866](https://github.com/hickoryai/kory-cli/pull/11866))
- goose-sdk version bump alpha 8 ([#11815](https://github.com/hickoryai/kory-cli/pull/11815))
- generate ACP reference documentation from release schemas ([#11770](https://github.com/hickoryai/kory-cli/pull/11770))
- publish GDK packages from version tags ([#11595](https://github.com/hickoryai/kory-cli/pull/11595))
- *(acp)* remove unused provider list fields ([#11696](https://github.com/hickoryai/kory-cli/pull/11696))
- *(acp)* remove unused unstable methods ([#11650](https://github.com/hickoryai/kory-cli/pull/11650))
- add SDK API reference for Rust, Python, and Kotlin ([#11251](https://github.com/hickoryai/kory-cli/pull/11251))
- pin V1 SDK and narrow acp crate features ([#11289](https://github.com/hickoryai/kory-cli/pull/11289))
- Migrate session/delete to the standard acp sdk ([#11286](https://github.com/hickoryai/kory-cli/pull/11286))
- Improve ACP provider setup and unify setup metadata ([#11100](https://github.com/hickoryai/kory-cli/pull/11100))
- create the goose-agent crate with the unrolled agent loop state machine ([#11216](https://github.com/hickoryai/kory-cli/pull/11216))
- Revert "feat(acp): add _goose/unstable/session/extensions/apply method ([#11081](https://github.com/hickoryai/kory-cli/pull/11081))" ([#11217](https://github.com/hickoryai/kory-cli/pull/11217))
- enhance the uniffi API layer ([#10427](https://github.com/hickoryai/kory-cli/pull/10427))
- Remove stale crates/goose-server and update docs ([#10224](https://github.com/hickoryai/kory-cli/pull/10224))
- Switch the local inference provider MLX backend to use the safemlx crate ([#10304](https://github.com/hickoryai/kory-cli/pull/10304))
- add provider bindings MVP to goose-sdk, and add python wheel publishing ([#10208](https://github.com/hickoryai/kory-cli/pull/10208))
- Migrate local inference model management to ACP ([#10124](https://github.com/hickoryai/kory-cli/pull/10124))
- *(acp)* upgrade SDK and use new HTTP/WS crate ([#10082](https://github.com/hickoryai/kory-cli/pull/10082))
- add ACP+ handlers for prompt editing ([#10031](https://github.com/hickoryai/kory-cli/pull/10031))
- Migrate Nostr session sharing to ACP ([#10022](https://github.com/hickoryai/kory-cli/pull/10022))
- ACP Migration: MCP/Goose Apps ([#9988](https://github.com/hickoryai/kory-cli/pull/9988))
- created custom methods for agent mention and slash command ([#9980](https://github.com/hickoryai/kory-cli/pull/9980))
- Migrate diagnostics to JSON report ([#9964](https://github.com/hickoryai/kory-cli/pull/9964))
- expose ACP thinking effort config option ([#9711](https://github.com/hickoryai/kory-cli/pull/9711))

## [0.1.0-alpha.12](https://github.com/aaif-goose/goose/compare/gdk-v0.1.0-alpha.11...gdk-v0.1.0-alpha.12) - 2026-10-13

### Other

- Remove Nostr session sharing from goose core ([#12453](https://github.com/aaif-goose/goose/pull/12453))

## [0.1.0-alpha.10](https://github.com/aaif-goose/goose/compare/gdk-v0.1.0-alpha.9...gdk-v0.1.0-alpha.10) - 2026-09-24

### Added

- add Live voice conversations to the desktop app ([#12093](https://github.com/aaif-goose/goose/pull/12093))
- *(acp)* scope Toolshim to custom providers ([#11414](https://github.com/aaif-goose/goose/pull/11414))

### Fixed

- *(acp)* bind MCP app tools to extension owners ([#11416](https://github.com/aaif-goose/goose/pull/11416))
