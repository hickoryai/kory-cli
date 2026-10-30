# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0-alpha.13](https://github.com/hickoryai/kory-cli/compare/gdk-v0.1.0-alpha.12...gdk-v0.1.0-alpha.13) - 2026-10-30

### Added

- add IO Intelligence provider ([#12795](https://github.com/hickoryai/kory-cli/pull/12795))
- surface provider x-request-id on responses and errors ([#12587](https://github.com/hickoryai/kory-cli/pull/12587))
- add AIgateway declarative provider ([#12528](https://github.com/hickoryai/kory-cli/pull/12528))
- *(providers)* honor supports_vision declared on custom provider mo… ([#12769](https://github.com/hickoryai/kory-cli/pull/12769))
- Use live model metadata for OpenAI reasoning and prefer the Responses API ([#12568](https://github.com/hickoryai/kory-cli/pull/12568))
- *(gdk)* add model discovery to the Provider API ([#12497](https://github.com/hickoryai/kory-cli/pull/12497))
- *(providers)* add Z.AI Coding Plan with streaming tool calls ([#12205](https://github.com/hickoryai/kory-cli/pull/12205))
- *(sdk)* allow OpenAI provider to accept a custom base URL ([#11868](https://github.com/hickoryai/kory-cli/pull/11868)) ([#11967](https://github.com/hickoryai/kory-cli/pull/11967))
- add Live voice conversations to the desktop app ([#12093](https://github.com/hickoryai/kory-cli/pull/12093))
- *(providers)* add EUrouter as a declarative provider ([#11619](https://github.com/hickoryai/kory-cli/pull/11619))
- *(acp)* scope Toolshim to custom providers ([#11414](https://github.com/hickoryai/kory-cli/pull/11414))
- *(gdk)* let Kotlin callers configure the Databricks AI Gateway path ([#11631](https://github.com/hickoryai/kory-cli/pull/11631))
- *(gdk)* add first-class document content support ([#11629](https://github.com/hickoryai/kory-cli/pull/11629))
- *(auth)* enable refresh auth cli ([#11657](https://github.com/hickoryai/kory-cli/pull/11657))
- *(providers)* add Opper as a declarative OpenAI-compatible provider ([#11589](https://github.com/hickoryai/kory-cli/pull/11589))
- *(provider)* discover Databricks Unity Catalog model services ([#11575](https://github.com/hickoryai/kory-cli/pull/11575))
- *(providers)* add TrustedRouter declarative provider ([#11422](https://github.com/hickoryai/kory-cli/pull/11422))
- *(providers)* add OpenCode Zen gateway as a declarative provider ([#10939](https://github.com/hickoryai/kory-cli/pull/10939))
- *(otel)* add request params, response metadata, tool call parity, and agent identification ([#11261](https://github.com/hickoryai/kory-cli/pull/11261))
- *(providers)* add SayGM as declarative OpenAI-compatible provider ([#11267](https://github.com/hickoryai/kory-cli/pull/11267))
- *(providers)* add Lynkr as a declarative OpenAI-compatible provider ([#11372](https://github.com/hickoryai/kory-cli/pull/11372))
- *(providers)* add PleumRouter declarative provider ([#10479](https://github.com/hickoryai/kory-cli/pull/10479))
- *(providers)* replace Z.ai GLM-5.2 with GLM-5.3 ([#11226](https://github.com/hickoryai/kory-cli/pull/11226))
- add Celeris provider ([#10714](https://github.com/hickoryai/kory-cli/pull/10714))
- *(provider)* add Friendli as declarative provider ([#10762](https://github.com/hickoryai/kory-cli/pull/10762))
- support latest Gemini models ([#10630](https://github.com/hickoryai/kory-cli/pull/10630))
- *(metamodels)* Add support for Muse Spark 1.1 via the Meta Models API ([#10432](https://github.com/hickoryai/kory-cli/pull/10432))
- *(ovhcloud)* add Qwen3.6-27B model (262K context, \$0.47/\$3.19 pe… ([#10339](https://github.com/hickoryai/kory-cli/pull/10339))
- *(providers)* add OllamaCloudProvider with dynamic model discovery and context limits ([#10264](https://github.com/hickoryai/kory-cli/pull/10264))
- per-message usage/cost tracking with derived session totals ([#10172](https://github.com/hickoryai/kory-cli/pull/10172))
- track cache tokens for accurate cost reporting ([#9752](https://github.com/hickoryai/kory-cli/pull/9752))
- *(azure)* support Entra ID bearer token auth via AZURE_OPENAI_AD_TOKEN ([#9716](https://github.com/hickoryai/kory-cli/pull/9716))
- propagate elicitation decline and cancel actions end-to-end ([#9437](https://github.com/hickoryai/kory-cli/pull/9437))
- surface Anthropic stream refusals as visible errors ([#9724](https://github.com/hickoryai/kory-cli/pull/9724))
- steering messages with ACP ([#9560](https://github.com/hickoryai/kory-cli/pull/9560))

### Fixed

- *(providers)* add missing GLM models to OpenCode Go and Zen ([#12057](https://github.com/hickoryai/kory-cli/pull/12057))
- *(openrouter)* use the context length OpenRouter reports ([#12853](https://github.com/hickoryai/kory-cli/pull/12853))
- *(openai)* send session id as prompt_cache_key on Responses requests ([#12420](https://github.com/hickoryai/kory-cli/pull/12420))
- *(openrouter)* do not set transforms field unconditionally ([#11374](https://github.com/hickoryai/kory-cli/pull/11374))
- *(anthropic)* forward unknown request_params to wire, mirroring openai engine ([#12720](https://github.com/hickoryai/kory-cli/pull/12720))
- *(provider)* passing thinking effort to ollama models ([#12555](https://github.com/hickoryai/kory-cli/pull/12555))
- *(costs)* estimate through canonical provider aliases ([#11916](https://github.com/hickoryai/kory-cli/pull/11916))
- *(databricks_v2)* send Claude model services through the Anthropic route ([#12486](https://github.com/hickoryai/kory-cli/pull/12486))
- *(gdk)* resolve vision support from the canonical catalog so tool result images reach the model ([#12440](https://github.com/hickoryai/kory-cli/pull/12440))
- *(providers)* rename deepseek-v4-flash to deepseek-flash ([#12269](https://github.com/hickoryai/kory-cli/pull/12269))
- *(providers)* classify many-image dimension limit errors as context length-exceeded ([#12208](https://github.com/hickoryai/kory-cli/pull/12208))
- *(providers)* drive pre-key model lists from the canonical registry ([#11475](https://github.com/hickoryai/kory-cli/pull/11475))
- fix Databricks GLM-5.3 and Kimi K3 reasoning effort ([#12079](https://github.com/hickoryai/kory-cli/pull/12079))
- *(anthropic)* preserved-thinking compliance for the provider layer ([#11836](https://github.com/hickoryai/kory-cli/pull/11836))
- send session ID header for OpenCode Go ([#11944](https://github.com/hickoryai/kory-cli/pull/11944))
- *(security)* require HTTPS for Snowflake ([#11745](https://github.com/hickoryai/kory-cli/pull/11745))
- *(providers)* correct zai default model id and add glm-5.3-flash ([#11674](https://github.com/hickoryai/kory-cli/pull/11674))
- unify context limit resolution behind provider API ([#11213](https://github.com/hickoryai/kory-cli/pull/11213))
- Avoid sending images to non vision capable models ([#11496](https://github.com/hickoryai/kory-cli/pull/11496))
- *(providers)* bound non-streaming JSON responses ([#11109](https://github.com/hickoryai/kory-cli/pull/11109))
- *(security)* bind Foundry API keys to request origin ([#11347](https://github.com/hickoryai/kory-cli/pull/11347))
- gate clear_thinking behind an opt-in flag (Z.AI-only) ([#10439](https://github.com/hickoryai/kory-cli/pull/10439))
- fix (providers): handle error code for context length exceeded ([#11283](https://github.com/hickoryai/kory-cli/pull/11283))
- *(providers)* stop paying the prompt-cache write premium on one-shot fast-model calls ([#11179](https://github.com/hickoryai/kory-cli/pull/11179))
- *(providers)* classify byte-size request limit errors as context-length-exceeded ([#11173](https://github.com/hickoryai/kory-cli/pull/11173))
- secure Copilot API endpoint transport ([#11129](https://github.com/hickoryai/kory-cli/pull/11129))
- *(providers)* drop stale signed thinking blocks after a mid-conversation model switch ([#10007](https://github.com/hickoryai/kory-cli/pull/10007))
- keep turn-context in place for OpenAI Responses-stack models so prompt caching works ([#10993](https://github.com/hickoryai/kory-cli/pull/10993))
- *(providers)* stop killing streaming responses at the total request timeout ([#10620](https://github.com/hickoryai/kory-cli/pull/10620))
- *(anthropic)* add claude-sonnet-5 and claude-fable-5 to known models list ([#10865](https://github.com/hickoryai/kory-cli/pull/10865))
- honor timeout_seconds on the Anthropic declarative provider ([#10440](https://github.com/hickoryai/kory-cli/pull/10440))
- *(cerebras)* preserve thinking inline for models that reject reasoning_content ([#10774](https://github.com/hickoryai/kory-cli/pull/10774))
- *(together)* parse model list response ([#10576](https://github.com/hickoryai/kory-cli/pull/10576))
- *(providers)* refresh Moonshot model list with current Kimi lineup ([#10696](https://github.com/hickoryai/kory-cli/pull/10696))
- *(providers)* update DeepSeek model names to v4 API ([#10729](https://github.com/hickoryai/kory-cli/pull/10729))
- *(provider)* add MiniMax-M3 and missing M2.7 variants to minimax provider ([#10431](https://github.com/hickoryai/kory-cli/pull/10431))
- *(providers)* add claude-sonnet-5 to canonical model registry ([#10183](https://github.com/hickoryai/kory-cli/pull/10183))
- tolerate partial Responses output items ([#10108](https://github.com/hickoryai/kory-cli/pull/10108))
- *(providers)* unescape shell-escaped image paths ([#10098](https://github.com/hickoryai/kory-cli/pull/10098))
- *(bedrock)* send inference config (max_tokens, temperature) on Converse ([#9889](https://github.com/hickoryai/kory-cli/pull/9889))
- *(providers)* don't retry deterministically-permanent 400s (thinking-block immutability) ([#10005](https://github.com/hickoryai/kory-cli/pull/10005))
- *(deps)* downgrade pkcs8 to v0.10 to match sec1/pkcs1 v0.7 ([#10119](https://github.com/hickoryai/kory-cli/pull/10119))
- *(providers)* reject non-object tool-call arguments instead of panicking ([#9832](https://github.com/hickoryai/kory-cli/pull/9832))
- *(providers)* Buffer thinking across stream chunks for DeepSeek/Kimi tool calls ([#9704](https://github.com/hickoryai/kory-cli/pull/9704))
- *(cost)* resolve databricks_v2 pricing and surface cost in standard usage update ([#9925](https://github.com/hickoryai/kory-cli/pull/9925))
- record OpenAI-native cached_tokens in usage metering ([#9829](https://github.com/hickoryai/kory-cli/pull/9829))
- *(providers)* detect image paths with spaces ([#9387](https://github.com/hickoryai/kory-cli/pull/9387))
- correctly map ollama_cloud to canonical provider and update max_… ([#9639](https://github.com/hickoryai/kory-cli/pull/9639))

### Other

- Support provider-defined cache prices ([#12944](https://github.com/hickoryai/kory-cli/pull/12944))
- *(release)* bump version to 1.55.0 (minor) ([#12863](https://github.com/hickoryai/kory-cli/pull/12863))
- *(GDK)* release v0.1.0-alpha.12 ([#12561](https://github.com/hickoryai/kory-cli/pull/12561))
- detect vision support ([#12594](https://github.com/hickoryai/kory-cli/pull/12594))
- Run the GDK agent loop on wasm32 ([#12569](https://github.com/hickoryai/kory-cli/pull/12569))
- Add Meta Muse Code provider ([#11765](https://github.com/hickoryai/kory-cli/pull/11765))
- *(GDK)* release v0.1.0-alpha.11 ([#12495](https://github.com/hickoryai/kory-cli/pull/12495))
- *(GDK)* release v0.1.0-alpha.10 ([#12492](https://github.com/hickoryai/kory-cli/pull/12492))
- Add a decisions provider crate, with impls for openrouter and jev ([#12418](https://github.com/hickoryai/kory-cli/pull/12418))
- gpt-live API support ([#12011](https://github.com/hickoryai/kory-cli/pull/12011))
- *(GDK)* release v0.1.0-alpha.9 ([#11866](https://github.com/hickoryai/kory-cli/pull/11866))
- Support GPT-6 Astra models ([#11869](https://github.com/hickoryai/kory-cli/pull/11869))
- goose-sdk version bump alpha 8 ([#11815](https://github.com/hickoryai/kory-cli/pull/11815))
- *(local-inference)* remove managed model registry ([#11789](https://github.com/hickoryai/kory-cli/pull/11789))
- Integration request from AIML API ([#11758](https://github.com/hickoryai/kory-cli/pull/11758))
- remove fast model routing ([#11469](https://github.com/hickoryai/kory-cli/pull/11469))
- publish GDK packages from version tags ([#11595](https://github.com/hickoryai/kory-cli/pull/11595))
- migrate openrouter provider to goose-providers ([#11757](https://github.com/hickoryai/kory-cli/pull/11757))
- add SDK API reference for Rust, Python, and Kotlin ([#11251](https://github.com/hickoryai/kory-cli/pull/11251))
- *(providers)* isolate environment-proxy test in its own binary ([#11262](https://github.com/hickoryai/kory-cli/pull/11262))
- *(deps)* bump pkcs8 from 0.10.2 to 0.11.0 ([#11246](https://github.com/hickoryai/kory-cli/pull/11246))
- Improve ACP provider setup and unify setup metadata ([#11100](https://github.com/hickoryai/kory-cli/pull/11100))
- create the goose-agent crate with the unrolled agent loop state machine ([#11216](https://github.com/hickoryai/kory-cli/pull/11216))
- Cache-safe request assembly: append-only turn context and declared cache semantics ([#11022](https://github.com/hickoryai/kory-cli/pull/11022))
- fall back to static model list only for non-models payloads ([#10189](https://github.com/hickoryai/kory-cli/pull/10189))
- *(deps)* bump pem from 3.0.6 to 4.0.0 ([#10853](https://github.com/hickoryai/kory-cli/pull/10853))
- Add Azure AI Foundry multi-LLM provider ([#10622](https://github.com/hickoryai/kory-cli/pull/10622))
- *(release)* bump version to 1.45.0 (minor) ([#10750](https://github.com/hickoryai/kory-cli/pull/10750))
- opus5 support (including adaptive thinking) ([#10686](https://github.com/hickoryai/kory-cli/pull/10686))
- enhance the uniffi API layer ([#10427](https://github.com/hickoryai/kory-cli/pull/10427))
- Add declarative Sakana AI provider for the OpenAI-compatible Fugu API ([#10357](https://github.com/hickoryai/kory-cli/pull/10357))
- remove unused utoipa ToSchema derives ([#10505](https://github.com/hickoryai/kory-cli/pull/10505))
- Add GPT-5.6 support ([#10384](https://github.com/hickoryai/kory-cli/pull/10384))
- remove unavailable models from cerebras catalog ([#10407](https://github.com/hickoryai/kory-cli/pull/10407))
- Add GPT-5.6 family support ([#10349](https://github.com/hickoryai/kory-cli/pull/10349))
- Disable thinking preservation for Groq ([#10284](https://github.com/hickoryai/kory-cli/pull/10284))
- move google provider into goose-providers ([#10216](https://github.com/hickoryai/kory-cli/pull/10216))
- add provider bindings MVP to goose-sdk, and add python wheel publishing ([#10208](https://github.com/hickoryai/kory-cli/pull/10208))
- move snowflake ([#10218](https://github.com/hickoryai/kory-cli/pull/10218))
- move databricks providers into goose-providers crate ([#10162](https://github.com/hickoryai/kory-cli/pull/10162))
- Move declarative definitions ([#10167](https://github.com/hickoryai/kory-cli/pull/10167))
- Refactor local inference provider crates ([#10169](https://github.com/hickoryai/kory-cli/pull/10169))
- add declarative provider support to goose-providers crate ([#9992](https://github.com/hickoryai/kory-cli/pull/9992))
- Make OpenAI Responses API store param configurable ([#10040](https://github.com/hickoryai/kory-cli/pull/10040))
- *(release)* bump version to 1.40.0 (minor) ([#10099](https://github.com/hickoryai/kory-cli/pull/10099))
- move ollama provider into goose-providers ([#9986](https://github.com/hickoryai/kory-cli/pull/9986))
- *(providers)* keep turn-context out of the Anthropic prefix cache ([#10030](https://github.com/hickoryai/kory-cli/pull/10030))
- Remove session_id from provider streaming trait methods ([#9984](https://github.com/hickoryai/kory-cli/pull/9984))
- Support streamed tool calls without indexes ([#10023](https://github.com/hickoryai/kory-cli/pull/10023))
- move anthropic provider into goose-providers ([#9985](https://github.com/hickoryai/kory-cli/pull/9985))
- surface actionable errors for truncated tool-call arguments ([#9946](https://github.com/hickoryai/kory-cli/pull/9946))
- provider refactor: don't require a model config to create a provider ([#9953](https://github.com/hickoryai/kory-cli/pull/9953))
- *(release)* bump version to 1.39.0 (minor) ([#9810](https://github.com/hickoryai/kory-cli/pull/9810))
- move OpenAI provider into goose-providers ([#9880](https://github.com/hickoryai/kory-cli/pull/9880))
- *(deps)* bump pkcs8 from 0.10.2 to 0.11.0 ([#9903](https://github.com/hickoryai/kory-cli/pull/9903))
- move ApiClient to goose-providers, lifting out TlsConfig ([#9869](https://github.com/hickoryai/kory-cli/pull/9869))
- refactor the request logger out of providers ([#9847](https://github.com/hickoryai/kory-cli/pull/9847))
- delete embeddings support ([#9865](https://github.com/hickoryai/kory-cli/pull/9865))
- move the provider trait into goose-providers ([#9860](https://github.com/hickoryai/kory-cli/pull/9860))
- move global config access out of ModelConfig, and move ModelConfig into goose_providers ([#9769](https://github.com/hickoryai/kory-cli/pull/9769))
- Add support for MLX models to the local inference provider ([#9154](https://github.com/hickoryai/kory-cli/pull/9154))
- *(release)* bump version to 1.38.0 (minor) ([#9684](https://github.com/hickoryai/kory-cli/pull/9684))
- Mark stream decode errors retryable ([#9723](https://github.com/hickoryai/kory-cli/pull/9723))
- Add canonical thinking modes ([#9743](https://github.com/hickoryai/kory-cli/pull/9743))
- refresh canonical model registry ([#9709](https://github.com/hickoryai/kory-cli/pull/9709))
- move formats/openai.rs into goose-providers crate, along with several dependencies ([#9633](https://github.com/hickoryai/kory-cli/pull/9633))
- create goose-providers crate with canonical models, conversation and other types ([#9588](https://github.com/hickoryai/kory-cli/pull/9588))

## [0.1.0-alpha.12](https://github.com/aaif-goose/goose/compare/gdk-v0.1.0-alpha.11...gdk-v0.1.0-alpha.12) - 2026-10-13

### Added

- surface provider x-request-id on responses and errors ([#12587](https://github.com/aaif-goose/goose/pull/12587))
- add AIgateway declarative provider ([#12528](https://github.com/aaif-goose/goose/pull/12528))
- *(providers)* honor supports_vision declared on custom provider mo… ([#12769](https://github.com/aaif-goose/goose/pull/12769))
- Use live model metadata for OpenAI reasoning and prefer the Responses API ([#12568](https://github.com/aaif-goose/goose/pull/12568))

### Fixed

- *(openrouter)* do not set transforms field unconditionally ([#11374](https://github.com/aaif-goose/goose/pull/11374))
- *(anthropic)* forward unknown request_params to wire, mirroring openai engine ([#12720](https://github.com/aaif-goose/goose/pull/12720))

### Other

- detect vision support ([#12594](https://github.com/aaif-goose/goose/pull/12594))
- Run the GDK agent loop on wasm32 ([#12569](https://github.com/aaif-goose/goose/pull/12569))
- Add Meta Muse Code provider ([#11765](https://github.com/aaif-goose/goose/pull/11765))

## [0.1.0-alpha.11](https://github.com/aaif-goose/goose/compare/gdk-v0.1.0-alpha.10...gdk-v0.1.0-alpha.11) - 2026-09-28

### Added

- *(gdk)* add model discovery to the Provider API ([#12497](https://github.com/aaif-goose/goose/pull/12497))

### Fixed

- *(provider)* passing thinking effort to ollama models ([#12555](https://github.com/aaif-goose/goose/pull/12555))
- *(costs)* estimate through canonical provider aliases ([#11916](https://github.com/aaif-goose/goose/pull/11916))
- *(databricks_v2)* send Claude model services through the Anthropic route ([#12486](https://github.com/aaif-goose/goose/pull/12486))

## [0.1.0-alpha.10](https://github.com/aaif-goose/goose/compare/gdk-v0.1.0-alpha.9...gdk-v0.1.0-alpha.10) - 2026-09-24

### Added

- *(providers)* add Z.AI Coding Plan with streaming tool calls ([#12205](https://github.com/aaif-goose/goose/pull/12205))
- *(sdk)* allow OpenAI provider to accept a custom base URL ([#11868](https://github.com/aaif-goose/goose/pull/11868)) ([#11967](https://github.com/aaif-goose/goose/pull/11967))
- add Live voice conversations to the desktop app ([#12093](https://github.com/aaif-goose/goose/pull/12093))
- *(providers)* add EUrouter as a declarative provider ([#11619](https://github.com/aaif-goose/goose/pull/11619))
- *(acp)* scope Toolshim to custom providers ([#11414](https://github.com/aaif-goose/goose/pull/11414))

### Fixed

- *(gdk)* resolve vision support from the canonical catalog so tool result images reach the model ([#12440](https://github.com/aaif-goose/goose/pull/12440))
- *(providers)* rename deepseek-v4-flash to deepseek-flash ([#12269](https://github.com/aaif-goose/goose/pull/12269))
- *(providers)* classify many-image dimension limit errors as context length-exceeded ([#12208](https://github.com/aaif-goose/goose/pull/12208))
- *(providers)* drive pre-key model lists from the canonical registry ([#11475](https://github.com/aaif-goose/goose/pull/11475))
- fix Databricks GLM-5.3 and Kimi K3 reasoning effort ([#12079](https://github.com/aaif-goose/goose/pull/12079))
- *(anthropic)* preserved-thinking compliance for the provider layer ([#11836](https://github.com/aaif-goose/goose/pull/11836))
- send session ID header for OpenCode Go ([#11944](https://github.com/aaif-goose/goose/pull/11944))

### Other

- Add a decisions provider crate, with impls for openrouter and jev ([#12418](https://github.com/aaif-goose/goose/pull/12418))
- gpt-live API support ([#12011](https://github.com/aaif-goose/goose/pull/12011))

## [0.1.0-alpha.9](https://github.com/aaif-goose/goose/compare/gdk-v0.1.0-alpha.8...gdk-v0.1.0-alpha.9) - 2026-09-08

### Fixed

- *(security)* require HTTPS for Snowflake ([#11745](https://github.com/aaif-goose/goose/pull/11745))

### Other

- Support GPT-6 Astra models ([#11869](https://github.com/aaif-goose/goose/pull/11869))
