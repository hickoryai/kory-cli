# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0-alpha.13](https://github.com/hickoryai/kory-cli/compare/gdk-v0.1.0-alpha.12...gdk-v0.1.0-alpha.13) - 2026-10-29

### Added

- surface provider x-request-id on responses and errors ([#12587](https://github.com/hickoryai/kory-cli/pull/12587))
- *(providers)* honor supports_vision declared on custom provider mo… ([#12769](https://github.com/hickoryai/kory-cli/pull/12769))
- Use live model metadata for OpenAI reasoning and prefer the Responses API ([#12568](https://github.com/hickoryai/kory-cli/pull/12568))
- fetch model metadata from models.dev live with bundled fallback ([#12560](https://github.com/hickoryai/kory-cli/pull/12560))
- *(gdk)* add model discovery to the Provider API ([#12497](https://github.com/hickoryai/kory-cli/pull/12497))
- *(gdk)* add first-class document content support ([#11629](https://github.com/hickoryai/kory-cli/pull/11629))
- *(gdk)* expose Anthropic response metadata for observability ([#11630](https://github.com/hickoryai/kory-cli/pull/11630))
- configurable Anthropic prompt-cache TTL (5m/1h) ([#11576](https://github.com/hickoryai/kory-cli/pull/11576))
- *(providers)* add OpenCode Zen gateway as a declarative provider ([#10939](https://github.com/hickoryai/kory-cli/pull/10939))
- *(otel)* add request params, response metadata, tool call parity, and agent identification ([#11261](https://github.com/hickoryai/kory-cli/pull/11261))
- *(acp)* derive and forward thinking effort from the ACP harness ([#10949](https://github.com/hickoryai/kory-cli/pull/10949))
- surface output-token-limit info ([#10831](https://github.com/hickoryai/kory-cli/pull/10831))
- *(providers)* forward images and MCP embedded-resource blobs in Anthropic and Google formats ([#10340](https://github.com/hickoryai/kory-cli/pull/10340))
- add stable agent event message identity ([#10716](https://github.com/hickoryai/kory-cli/pull/10716))
- support latest Gemini models ([#10630](https://github.com/hickoryai/kory-cli/pull/10630))
- per-message usage/cost tracking with derived session totals ([#10172](https://github.com/hickoryai/kory-cli/pull/10172))

### Fixed

- *(moonshot)* map provider name to moonshotai for canonical model lookup ([#12041](https://github.com/hickoryai/kory-cli/pull/12041))
- *(providers)* tolerate missing created_at on OpenAI Responses stream events ([#12241](https://github.com/hickoryai/kory-cli/pull/12241))
- *(cost)* apply long-context price tiers to cost estimates ([#12787](https://github.com/hickoryai/kory-cli/pull/12787))
- adaptive thinking for Claude Sonnet and Haiku 5.5 ([#12761](https://github.com/hickoryai/kory-cli/pull/12761))
- *(anthropic)* forward unknown request_params to wire, mirroring openai engine ([#12720](https://github.com/hickoryai/kory-cli/pull/12720))
- support hyphenated GPT 6.1 Sol effort aliases ([#12689](https://github.com/hickoryai/kory-cli/pull/12689))
- *(cost)* price Databricks GLM aliases from the zhipuai catalog ([#12673](https://github.com/hickoryai/kory-cli/pull/12673))
- *(bedrock)* preserve the service total when folding cache input tokens ([#12586](https://github.com/hickoryai/kory-cli/pull/12586))
- session naming/"none" reasoning effort fails for gpt-6.1-sol ([#12605](https://github.com/hickoryai/kory-cli/pull/12605))
- *(providers)* read cache write tokens from Responses API usage ([#12509](https://github.com/hickoryai/kory-cli/pull/12509))
- keep tool results consecutive when OpenAI-compatible tool images are emitted ([#12233](https://github.com/hickoryai/kory-cli/pull/12233))
- GDK tools with no args still advertise empty params input schema ([#12527](https://github.com/hickoryai/kory-cli/pull/12527))
- *(costs)* estimate through canonical provider aliases ([#11916](https://github.com/hickoryai/kory-cli/pull/11916))
- *(gdk)* resolve vision support from the canonical catalog so tool result images reach the model ([#12440](https://github.com/hickoryai/kory-cli/pull/12440))
- *(providers)* set strict:false on chat-completions tools ([#12272](https://github.com/hickoryai/kory-cli/pull/12272))
- *(providers)* show CoreWeave in bundled metadata ([#12149](https://github.com/hickoryai/kory-cli/pull/12149))
- *(providers)* drive pre-key model lists from the canonical registry ([#11475](https://github.com/hickoryai/kory-cli/pull/11475))
- fix Databricks GLM-5.3 and Kimi K3 reasoning effort ([#12079](https://github.com/hickoryai/kory-cli/pull/12079))
- *(anthropic)* preserved-thinking compliance for the provider layer ([#11836](https://github.com/hickoryai/kory-cli/pull/11836))
- stop mapping Astra Off to reasoning.effort none ([#11976](https://github.com/hickoryai/kory-cli/pull/11976))
- *(google)* pair functionResponse names with the preceding request ([#11888](https://github.com/hickoryai/kory-cli/pull/11888))
- *(security)* bind CLI session naming to trusted path ([#11787](https://github.com/hickoryai/kory-cli/pull/11787))
- *(cache)* anchor chat-payload breakpoints on the agentic tail ([#11429](https://github.com/hickoryai/kory-cli/pull/11429))
- *(openai)* update stored tool call name when later delta carries non-empty name ([#11750](https://github.com/hickoryai/kory-cli/pull/11750))
- *(gdk)* preserve tool-call indices in streaming responses ([#11637](https://github.com/hickoryai/kory-cli/pull/11637))
- *(providers)* parse OpenRouter nested cache_write_tokens in usage ([#11472](https://github.com/hickoryai/kory-cli/pull/11472))
- unify context limit resolution behind provider API ([#11213](https://github.com/hickoryai/kory-cli/pull/11213))
- Avoid sending images to non vision capable models ([#11496](https://github.com/hickoryai/kory-cli/pull/11496))
- *(security)* sanitize Unicode tags in MCP prompts ([#11453](https://github.com/hickoryai/kory-cli/pull/11453))
- *(agents)* fail fast when a recipe's structured response can't reach an ACP-bridged provider ([#11307](https://github.com/hickoryai/kory-cli/pull/11307))
- *(providers)* estimate cost for Azure Foundry models via inferred catalog pricing ([#11264](https://github.com/hickoryai/kory-cli/pull/11264))
- *(providers)* coalesce consecutive Thinking blocks in collect_stream ([#11317](https://github.com/hickoryai/kory-cli/pull/11317))
- *(google)* include thinking tokens in usage output_tokens ([#10354](https://github.com/hickoryai/kory-cli/pull/10354))
- gate clear_thinking behind an opt-in flag (Z.AI-only) ([#10439](https://github.com/hickoryai/kory-cli/pull/10439))
- *(security)* redact URLs in provider errors ([#11230](https://github.com/hickoryai/kory-cli/pull/11230))
- *(providers)* send explicit thinking disabled to adaptive Claude models ([#11177](https://github.com/hickoryai/kory-cli/pull/11177))
- *(providers)* include required annotations field on replayed output_text items ([#11263](https://github.com/hickoryai/kory-cli/pull/11263))
- *(providers)* stop paying the prompt-cache write premium on one-shot fast-model calls ([#11179](https://github.com/hickoryai/kory-cli/pull/11179))
- *(acp)* return authe error for acp provider from goose acp server ([#11202](https://github.com/hickoryai/kory-cli/pull/11202))
- *(acp)* ignores acp provider generated title and uses goose generate title ([#11197](https://github.com/hickoryai/kory-cli/pull/11197))
- *(providers)* bound local image reads ([#11114](https://github.com/hickoryai/kory-cli/pull/11114))
- *(provider-types)* bound partial thinking tags ([#11108](https://github.com/hickoryai/kory-cli/pull/11108))
- *(providers)* map kimi_code provider name and pass correct provider to create_request ([#11130](https://github.com/hickoryai/kory-cli/pull/11130))
- adjust rmcp::model::Meta ref ([#11107](https://github.com/hickoryai/kory-cli/pull/11107))
- *(conversation)* sanitize nested tool responses ([#10609](https://github.com/hickoryai/kory-cli/pull/10609))
- *(providers)* drop stale signed thinking blocks after a mid-conversation model switch ([#10007](https://github.com/hickoryai/kory-cli/pull/10007))
- *(acp)* resume provider-native sessions ([#10379](https://github.com/hickoryai/kory-cli/pull/10379))
- *(streaming)* skip metadata-only SSE frames instead of failing the turn ([#10942](https://github.com/hickoryai/kory-cli/pull/10942))
- keep turn-context in place for OpenAI Responses-stack models so prompt caching works ([#10993](https://github.com/hickoryai/kory-cli/pull/10993))
- *(openrouter)* stop silently ignoring thinking effort off ([#10991](https://github.com/hickoryai/kory-cli/pull/10991))
- fix implicit OAuth during model discovery ([#10929](https://github.com/hickoryai/kory-cli/pull/10929))
- keep ACP session naming out of live conversations ([#10963](https://github.com/hickoryai/kory-cli/pull/10963))
- *(providers)* stop killing streaming responses at the total request timeout ([#10620](https://github.com/hickoryai/kory-cli/pull/10620))
- *(cerebras)* preserve thinking inline for models that reject reasoning_content ([#10774](https://github.com/hickoryai/kory-cli/pull/10774))
- *(providers)* restore dynamic model discovery and current Grok support ([#10756](https://github.com/hickoryai/kory-cli/pull/10756))
- fix overlong function names in provider requests ([#10659](https://github.com/hickoryai/kory-cli/pull/10659))
- *(providers)* rewrite oneOf to anyOf in tool schemas for OpenAI-compatible backends ([#10571](https://github.com/hickoryai/kory-cli/pull/10571))
- *(acp)* preserve content audience boundaries ([#10493](https://github.com/hickoryai/kory-cli/pull/10493))
- treat empty-string finish_reason as non-terminal in streaming ([#10258](https://github.com/hickoryai/kory-cli/pull/10258))
- Dedupe duplicate signed thinking blocks to fix Anthropic 400 ([#10080](https://github.com/hickoryai/kory-cli/pull/10080)) ([#10083](https://github.com/hickoryai/kory-cli/pull/10083))
- recover malformed tool calls from GLM/Minimax models instead of rejecting them ([#10230](https://github.com/hickoryai/kory-cli/pull/10230))
- *(providers)* treat unicode punctuation as image path terminators ([#10106](https://github.com/hickoryai/kory-cli/pull/10106))
- don't bleed provider-specific request_params into delegate() sessions ([#9906](https://github.com/hickoryai/kory-cli/pull/9906))

### Other

- Merge official Goose main into the router ACP branch
- Support provider-defined cache prices ([#12944](https://github.com/hickoryai/kory-cli/pull/12944))
- *(release)* bump version to 1.55.0 (minor) ([#12863](https://github.com/hickoryai/kory-cli/pull/12863))
- *(GDK)* release v0.1.0-alpha.12 ([#12561](https://github.com/hickoryai/kory-cli/pull/12561))
- Delete the legacy agent loop ([#12760](https://github.com/hickoryai/kory-cli/pull/12760))
- use typed structs for openai message formatting ([#12725](https://github.com/hickoryai/kory-cli/pull/12725))
- Refresh the model catalog on the blocking pool ([#12727](https://github.com/hickoryai/kory-cli/pull/12727))
- *(release)* bump version to 1.54.0 (minor) ([#12701](https://github.com/hickoryai/kory-cli/pull/12701))
- upgrade rust to 1.99.0 ([#12714](https://github.com/hickoryai/kory-cli/pull/12714))
- *(release)* bump version to 1.53.0 (minor) ([#12565](https://github.com/hickoryai/kory-cli/pull/12565))
- Run the GDK agent loop on wasm32 ([#12569](https://github.com/hickoryai/kory-cli/pull/12569))
- Add Meta Muse Code provider ([#11765](https://github.com/hickoryai/kory-cli/pull/11765))
- *(GDK)* release v0.1.0-alpha.11 ([#12495](https://github.com/hickoryai/kory-cli/pull/12495))
- *(GDK)* release v0.1.0-alpha.10 ([#12492](https://github.com/hickoryai/kory-cli/pull/12492))
- support Opus 5.5, GPT-6-{sol,luna} ([#12447](https://github.com/hickoryai/kory-cli/pull/12447))
- *(release)* bump version to 1.52.0 (minor) ([#12421](https://github.com/hickoryai/kory-cli/pull/12421))
- compress canonical_models.json ([#12385](https://github.com/hickoryai/kory-cli/pull/12385))
- *(release)* bump version to 1.51.0 (minor) ([#12071](https://github.com/hickoryai/kory-cli/pull/12071))
- *(GDK)* release v0.1.0-alpha.9 ([#11866](https://github.com/hickoryai/kory-cli/pull/11866))
- *(release)* bump version to 1.50.0 (minor) ([#11907](https://github.com/hickoryai/kory-cli/pull/11907))
- Support GPT-6 Astra models ([#11869](https://github.com/hickoryai/kory-cli/pull/11869))
- goose-sdk version bump alpha 8 ([#11815](https://github.com/hickoryai/kory-cli/pull/11815))
- remove fast model routing ([#11469](https://github.com/hickoryai/kory-cli/pull/11469))
- publish GDK packages from version tags ([#11595](https://github.com/hickoryai/kory-cli/pull/11595))
- *(release)* bump version to 1.49.0 (minor) ([#11727](https://github.com/hickoryai/kory-cli/pull/11727))
- *(acp)* remove unused provider list fields ([#11696](https://github.com/hickoryai/kory-cli/pull/11696))
- Clean up obsolete extension paths before redesigning the extension manager ([#11645](https://github.com/hickoryai/kory-cli/pull/11645))
- update canonical models json ([#11641](https://github.com/hickoryai/kory-cli/pull/11641))
- *(release)* bump version to 1.48.0 (minor) ([#11542](https://github.com/hickoryai/kory-cli/pull/11542))
- add SDK API reference for Rust, Python, and Kotlin ([#11251](https://github.com/hickoryai/kory-cli/pull/11251))
- Add GPT-5.6 follow-up support for Codex and Responses API ([#10460](https://github.com/hickoryai/kory-cli/pull/10460))
- Bug fix: Goose unable to work with DeepSeek and Alibaba using OpenAI responses API. ([#11298](https://github.com/hickoryai/kory-cli/pull/11298))
- *(release)* bump version to 1.47.0 (minor) ([#11300](https://github.com/hickoryai/kory-cli/pull/11300))
- Improve ACP provider setup and unify setup metadata ([#11100](https://github.com/hickoryai/kory-cli/pull/11100))
- create the goose-agent crate with the unrolled agent loop state machine ([#11216](https://github.com/hickoryai/kory-cli/pull/11216))
- *(release)* bump version to 1.46.0 (minor) ([#10920](https://github.com/hickoryai/kory-cli/pull/10920))
- Sanitize Unicode tags in Responses output ([#10745](https://github.com/hickoryai/kory-cli/pull/10745))
- Cache-safe request assembly: append-only turn context and declared cache semantics ([#11022](https://github.com/hickoryai/kory-cli/pull/11022))
- Unrolled agent loop ([#9574](https://github.com/hickoryai/kory-cli/pull/9574))
- Sanitize Unicode tags in MCP resources ([#10746](https://github.com/hickoryai/kory-cli/pull/10746))
- *(providers)* keep turn-context out of the OpenAI-compatible prefix cache ([#10937](https://github.com/hickoryai/kory-cli/pull/10937))
- Add Azure AI Foundry multi-LLM provider ([#10622](https://github.com/hickoryai/kory-cli/pull/10622))
- *(release)* bump version to 1.45.0 (minor) ([#10750](https://github.com/hickoryai/kory-cli/pull/10750))
- upgrade to rmcp 2.0 ([#10584](https://github.com/hickoryai/kory-cli/pull/10584))
- opus5 support (including adaptive thinking) ([#10686](https://github.com/hickoryai/kory-cli/pull/10686))
- refactor (acp): simplify ACP tool-call handling and fix chain summaries ([#10599](https://github.com/hickoryai/kory-cli/pull/10599))
- enhance the uniffi API layer ([#10427](https://github.com/hickoryai/kory-cli/pull/10427))
- rebuild canonical model db ([#10633](https://github.com/hickoryai/kory-cli/pull/10633))
- *(release)* bump version to 1.44.0 (minor) ([#10597](https://github.com/hickoryai/kory-cli/pull/10597))
- remove unused utoipa ToSchema derives ([#10505](https://github.com/hickoryai/kory-cli/pull/10505))
- Add GPT-5.6 support ([#10384](https://github.com/hickoryai/kory-cli/pull/10384))
- *(release)* bump version to 1.43.0 (minor) ([#10434](https://github.com/hickoryai/kory-cli/pull/10434))
- sanitize responses function call names ([#10344](https://github.com/hickoryai/kory-cli/pull/10344))
- Update canonical model inventory ([#10352](https://github.com/hickoryai/kory-cli/pull/10352))
- Add GPT-5.6 family support ([#10349](https://github.com/hickoryai/kory-cli/pull/10349))
- *(release)* bump version to 1.42.0 (minor) ([#10295](https://github.com/hickoryai/kory-cli/pull/10295))
- Update Rust toolchain and raise recursion limit ([#10303](https://github.com/hickoryai/kory-cli/pull/10303))
- Switch the local inference provider MLX backend to use the safemlx crate ([#10304](https://github.com/hickoryai/kory-cli/pull/10304))
- move google provider into goose-providers ([#10216](https://github.com/hickoryai/kory-cli/pull/10216))
- add provider bindings MVP to goose-sdk, and add python wheel publishing ([#10208](https://github.com/hickoryai/kory-cli/pull/10208))
- move snowflake ([#10218](https://github.com/hickoryai/kory-cli/pull/10218))
- move databricks providers into goose-providers crate ([#10162](https://github.com/hickoryai/kory-cli/pull/10162))
- *(release)* bump version to 1.41.0 (minor) ([#10198](https://github.com/hickoryai/kory-cli/pull/10198))
- Fix canonical model output paths ([#10196](https://github.com/hickoryai/kory-cli/pull/10196))
- Refactor local inference provider crates ([#10169](https://github.com/hickoryai/kory-cli/pull/10169))

## [0.1.0-alpha.12](https://github.com/aaif-goose/goose/compare/gdk-v0.1.0-alpha.11...gdk-v0.1.0-alpha.12) - 2026-10-13

### Added

- surface provider x-request-id on responses and errors ([#12587](https://github.com/aaif-goose/goose/pull/12587))
- *(providers)* honor supports_vision declared on custom provider mo… ([#12769](https://github.com/aaif-goose/goose/pull/12769))
- Use live model metadata for OpenAI reasoning and prefer the Responses API ([#12568](https://github.com/aaif-goose/goose/pull/12568))
- fetch model metadata from models.dev live with bundled fallback ([#12560](https://github.com/aaif-goose/goose/pull/12560))

### Fixed

- *(cost)* apply long-context price tiers to cost estimates ([#12787](https://github.com/aaif-goose/goose/pull/12787))
- adaptive thinking for Claude Sonnet and Haiku 5.5 ([#12761](https://github.com/aaif-goose/goose/pull/12761))
- *(anthropic)* forward unknown request_params to wire, mirroring openai engine ([#12720](https://github.com/aaif-goose/goose/pull/12720))
- support hyphenated GPT 6.1 Sol effort aliases ([#12689](https://github.com/aaif-goose/goose/pull/12689))
- *(cost)* price Databricks GLM aliases from the zhipuai catalog ([#12673](https://github.com/aaif-goose/goose/pull/12673))
- *(bedrock)* preserve the service total when folding cache input tokens ([#12586](https://github.com/aaif-goose/goose/pull/12586))
- session naming/"none" reasoning effort fails for gpt-6.1-sol ([#12605](https://github.com/aaif-goose/goose/pull/12605))
- *(providers)* read cache write tokens from Responses API usage ([#12509](https://github.com/aaif-goose/goose/pull/12509))
- keep tool results consecutive when OpenAI-compatible tool images are emitted ([#12233](https://github.com/aaif-goose/goose/pull/12233))

### Other

- Delete the legacy agent loop ([#12760](https://github.com/aaif-goose/goose/pull/12760))
- use typed structs for openai message formatting ([#12725](https://github.com/aaif-goose/goose/pull/12725))
- Refresh the model catalog on the blocking pool ([#12727](https://github.com/aaif-goose/goose/pull/12727))
- *(release)* bump version to 1.54.0 (minor) ([#12701](https://github.com/aaif-goose/goose/pull/12701))
- upgrade rust to 1.99.0 ([#12714](https://github.com/aaif-goose/goose/pull/12714))
- *(release)* bump version to 1.53.0 (minor) ([#12565](https://github.com/aaif-goose/goose/pull/12565))
- Run the GDK agent loop on wasm32 ([#12569](https://github.com/aaif-goose/goose/pull/12569))
- Add Meta Muse Code provider ([#11765](https://github.com/aaif-goose/goose/pull/11765))

## [0.1.0-alpha.11](https://github.com/aaif-goose/goose/compare/gdk-v0.1.0-alpha.10...gdk-v0.1.0-alpha.11) - 2026-09-28

### Added

- *(gdk)* add model discovery to the Provider API ([#12497](https://github.com/aaif-goose/goose/pull/12497))

### Fixed

- GDK tools with no args still advertise empty params input schema ([#12527](https://github.com/aaif-goose/goose/pull/12527))
- *(costs)* estimate through canonical provider aliases ([#11916](https://github.com/aaif-goose/goose/pull/11916))

## [0.1.0-alpha.10](https://github.com/aaif-goose/goose/compare/gdk-v0.1.0-alpha.9...gdk-v0.1.0-alpha.10) - 2026-09-24

### Fixed

- *(gdk)* resolve vision support from the canonical catalog so tool result images reach the model ([#12440](https://github.com/aaif-goose/goose/pull/12440))
- *(providers)* set strict:false on chat-completions tools ([#12272](https://github.com/aaif-goose/goose/pull/12272))
- *(providers)* show CoreWeave in bundled metadata ([#12149](https://github.com/aaif-goose/goose/pull/12149))
- *(providers)* drive pre-key model lists from the canonical registry ([#11475](https://github.com/aaif-goose/goose/pull/11475))
- fix Databricks GLM-5.3 and Kimi K3 reasoning effort ([#12079](https://github.com/aaif-goose/goose/pull/12079))
- *(anthropic)* preserved-thinking compliance for the provider layer ([#11836](https://github.com/aaif-goose/goose/pull/11836))
- stop mapping Astra Off to reasoning.effort none ([#11976](https://github.com/aaif-goose/goose/pull/11976))
- *(google)* pair functionResponse names with the preceding request ([#11888](https://github.com/aaif-goose/goose/pull/11888))

### Other

- support Opus 5.5, GPT-6-{sol,luna} ([#12447](https://github.com/aaif-goose/goose/pull/12447))
- *(release)* bump version to 1.52.0 (minor) ([#12421](https://github.com/aaif-goose/goose/pull/12421))
- compress canonical_models.json ([#12385](https://github.com/aaif-goose/goose/pull/12385))
- *(release)* bump version to 1.51.0 (minor) ([#12071](https://github.com/aaif-goose/goose/pull/12071))

## [0.1.0-alpha.9](https://github.com/aaif-goose/goose/compare/gdk-v0.1.0-alpha.8...gdk-v0.1.0-alpha.9) - 2026-09-08

### Other

- *(release)* bump version to 1.50.0 (minor) ([#11907](https://github.com/aaif-goose/goose/pull/11907))
- Support GPT-6 Astra models ([#11869](https://github.com/aaif-goose/goose/pull/11869))
