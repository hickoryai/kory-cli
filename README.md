<div align="center">

# goose

_your native open source AI agent — desktop app, CLI, and API — for code, workflows, and everything in between_

<p align="center">
  <a href="https://opensource.org/licenses/Apache-2.0"
    ><img src="https://img.shields.io/badge/License-Apache_2.0-blue.svg"></a>
  <a href="https://discord.gg/n8R5VaWDAn"
    ><img src="https://img.shields.io/discord/1287729918100246654?logo=discord&logoColor=white&label=Join+Us&color=blueviolet" alt="Discord"></a>
  <a href="https://github.com/aaif-goose/goose/actions/workflows/ci.yml"
     ><img src="https://img.shields.io/github/actions/workflow/status/aaif-goose/goose/ci.yml?branch=main" alt="CI"></a>
  <a href="https://insights.linuxfoundation.org/project/goose"><img src="https://insights.linuxfoundation.org/api/badge/health-score?project=goose"></a>
  <a href="https://repology.org/project/goose-cli/versions"><img src="https://repology.org/badge/tiny-repos/goose-cli.svg" alt="Packaging status"></a>
</p>

<a href="https://trendshift.io/repositories/25298?utm_source=repository-badge&amp;utm_medium=badge&amp;utm_campaign=badge-repository-25298" target="_blank" rel="noopener noreferrer"><img src="https://trendshift.io/api/badge/repositories/25298" alt="aaif-goose%2Fgoose | Trendshift" width="250" height="55"/></a>

</div>


goose is a general-purpose AI agent that runs on your machine. Not just for code — use it for research, writing, automation, data analysis, or anything you need to get done.

A native desktop app for macOS, Linux, and Windows. A full CLI for terminal workflows. An API to embed it anywhere. Built in Rust for performance and portability.

In Kory's CLI, router context handoffs and model failovers append prominent red
dividers to the transcript. Earlier conversation remains in the session history
and scrollback; each divider identifies the transition without clearing the screen.

goose works with 15+ providers — Anthropic, OpenAI, Google, Ollama, OpenRouter, Azure, Bedrock, and more. Use API keys or your existing Claude, ChatGPT, or Gemini subscriptions via [ACP](https://goose-docs.ai/docs/guides/acp-providers). Connect to 70+ extensions via the [Model Context Protocol](https://modelcontextprotocol.io/) open standard.

goose is part of the [Agentic AI Foundation (AAIF)](https://aaif.io/) at the Linux Foundation.

# Get started

**[Download the desktop app](https://goose-docs.ai/docs/getting-started/installation)** for macOS, Linux, and Windows.

Or install the CLI:

```bash
curl -fsSL https://github.com/hickoryai/goose/releases/download/stable/download_cli.sh | bash
```

# Quick links
- [Quickstart](https://goose-docs.ai/docs/quickstart)
- [Installation](https://goose-docs.ai/docs/getting-started/installation)
- [Tutorials](https://goose-docs.ai/docs/category/tutorials)
- [Documentation](https://goose-docs.ai/docs/category/getting-started)
- [Governance](https://github.com/aaif-goose/goose/blob/main/GOVERNANCE.md)
- [Custom Distributions](https://github.com/aaif-goose/goose/blob/main/CUSTOM_DISTROS.md) — build your own goose distro with preconfigured providers, extensions, and branding

## CLI activity footer

Router sessions pin the current account on the left of the header and fleet status
on the right. The current account shows session-cap consumption when available,
otherwise weekly consumption: `claude: zane@chordzy.com (45%)`. A matching capacity
reserve adds the consumption ceiling: `(45%/80%)` means a 20% session reserve.
The header reads router-acp's normalized account usage; model-scoped and spend caps
are excluded, and multiple pools use the highest consumption for the selected window
kind. Missing usage stays blank. Account data refreshes every ten seconds, with the
last successful reading retained if a query fails. Narrow terminals prioritize the
current account.

The router session's footer reads the same `session_info_update` activity state as the
terminal title (`_meta.routerAcp.busy`). Router-reported starts and stops control the
timer, including autonomous wakes. Output pauses and returning to the local prompt do
not mark the session stopped. The duration and local stop time are recorded when those
router notifications arrive. Child sessions read their lifecycle from the existing
`state-query v1 delegates` result. Other providers use the local interactive turn.

The context bar refreshes from the provider every three seconds for the entire footer
lifetime, including autonomous router turns while the input prompt is waiting. It follows
the current context size after handoff or compaction, including decreases; cumulative turn
tokens are never substituted for ACP context usage. Child-session queries do not delay it.

ACP terminal exit waits and releases run outside the incoming message handler, so a
running background command cannot block the output and kill requests needed to interrupt it.

The shutdown screen handles Ctrl-C from both operating-system signals and extended
keyboard input through the same two-press confirmation. Terminal key events must not
leave a saved, drained session unable to exit while a protocol response is delayed.

The terminal relay and prompt key listener enable newline translation on the session's
pseudo-terminal, including when the preceding input reader left it in raw mode. Transcript
lines therefore return to the left margin while the footer keeps accepting input.

Drag to select transcript text; right-click copies the selection and clears its highlight
without moving the view. Copying uses the local system clipboard as well as OSC 52 for
terminals that support clipboard forwarding, including remote sessions.

## Need help?
- [Diagnostics & Reporting](https://goose-docs.ai/docs/troubleshooting/diagnostics-and-reporting)
- [Known Issues](https://goose-docs.ai/docs/troubleshooting/known-issues)

# a little goose humor 🪿

> Why did the developer choose goose as their AI agent?
> 
> Because it always helps them "migrate" their code to production! 🚀

# goose around with us
- [Discord](https://discord.gg/n8R5VaWDAn)
- [YouTube](https://www.youtube.com/@goose-oss)
- [LinkedIn](https://www.linkedin.com/company/goose-oss)
- [Twitter/X](https://x.com/goose_oss)
