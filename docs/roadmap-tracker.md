# Roadmap Implementation Tracker

This document tracks the recommended implementation order for the Tekton roadmap. Use it to pick up where you left off across sessions.

## Recent progress

### `add_stablecoin_nix_config` branch (merged)

Major architectural change: **decoupled Tekton from specific repos**. Previews are now fully driven by a per-repo `preview-config.nix` rather than hardcoded configurations.

Key changes:
- Each repo ships `preview-config.nix` at its root describing how to build and run the app
- Tekton fetches the config from GitHub, builds a NixOS closure (cached by commit SHA), and runs the container
- Removed hardcoded `vertex-preview-config.nix` and `--type node|vertex` flag from all layers (preview.sh, webhook, dashboard backend/frontend, deploy scripts, setup scripts)
- Added structured `meta` block: `setupService`, `appServices`, `database`, `routes`, `hostSecrets`, `extraHosts`
- Dynamic Caddy route generation based on `meta.routes`
- Nix closure caching by commit SHA (config cache + closure cache)
- GitHub token acquisition from webhook for authenticated repo access
- Added `docs/adding-a-new-service.md` and `example_service/` (Python/Flask) as reference
- Preview slugs now include repo name (e.g. `myapp-42` instead of just `42`)
- Fixed HMAC signature verification (use raw body, not re-serialized JSON)

**Roadmap impact:**
- [#30](https://github.com/lambdaclass/tekton/issues/30) Portable agent configuration — **partially addressed** (preview config pattern established; agent config still needed)
- [#53](https://github.com/lambdaclass/tekton/issues/53) Custom agent runtimes — **partially addressed** (pluggable preview runtimes done; agent runtimes still needed)
- [#85](https://github.com/lambdaclass/tekton/issues/85) Run real-world apps in preview environments — **partially addressed** (container-local DBs, multi-service routing, secret forwarding now supported)

## Implementation order (P0 — start here)

The dependency chain is: **persistence → identity/RBAC → policy engine → everything else**.

| # | Issue | Title | Status | Depends on |
|---|-------|-------|--------|------------|
| 1 | [#22](https://github.com/lambdaclass/tekton/issues/22) | Task and prompt persistence | **Done** | — |
| 2 | [#23](https://github.com/lambdaclass/tekton/issues/23) | Security and access control (RBAC) | **Done** | #22 |
| 3 | [#24](https://github.com/lambdaclass/tekton/issues/24) | Policy engine and sandbox controls | Not started | #22, #23 |
| 4 | [#25](https://github.com/lambdaclass/tekton/issues/25) | Fine-grained tool approval UX | Not started | #24 |
| 5 | [#26](https://github.com/lambdaclass/tekton/issues/26) | Secrets provenance and leak prevention | Not started | #23, #24 |
| 6 | [#28](https://github.com/lambdaclass/tekton/issues/28) | Multi-model support | Not started | #23 |
| 7 | [#29](https://github.com/lambdaclass/tekton/issues/29) | Repo intelligence and draft/plan mode | Not started | #22, #23, #28 |
| 8 | [#27](https://github.com/lambdaclass/tekton/issues/27) | Identity-safe PR authorship | Not started | #23 |
| 9 | [#30](https://github.com/lambdaclass/tekton/issues/30) | Portable agent configuration (AGENTS.md) | In progress (preview config done, agent config remaining) | #24 |

## Quick wins (interleave anytime)

These are independent of the P0 dependency chain and can be done as palette cleansers between larger items.

| Issue | Title | Status |
|-------|-------|--------|
| [#21](https://github.com/lambdaclass/tekton/issues/21) | Remove update button from UI | **Done** |
| [#19](https://github.com/lambdaclass/tekton/issues/19) | Fix screenshot capture in Nix VM | **Done** |
| [#20](https://github.com/lambdaclass/tekton/issues/20) | Surface VM startup logs in the UI | **Done** |
| [#82](https://github.com/lambdaclass/tekton/issues/82) | Custom task naming | **Done** |
| [#84](https://github.com/lambdaclass/tekton/issues/84) | Mark task as failed | Not started |
| [#81](https://github.com/lambdaclass/tekton/issues/81) | Branch selector dropdown | **Done** |
| [#83](https://github.com/lambdaclass/tekton/issues/83) | Task-to-PR traceability | **Done** |
| [#79](https://github.com/lambdaclass/tekton/issues/79) | Dynamic task names | **Done** |
| [#80](https://github.com/lambdaclass/tekton/issues/80) | One-click PR creation | **Done** |
| [#41](https://github.com/lambdaclass/tekton/issues/41) | Repo classifier fallback UX | **Done** |

## P1: Scale and collaboration (after P0)

| Issue | Title | Status |
|-------|-------|--------|
| [#39](https://github.com/lambdaclass/tekton/issues/39) | Conversational threads | Not started |
| [#33](https://github.com/lambdaclass/tekton/issues/33) | Queue management and priority | Not started |
| [#34](https://github.com/lambdaclass/tekton/issues/34) | Cost tracking and budgets | Not started |
| [#35](https://github.com/lambdaclass/tekton/issues/35) | Audit log baseline | Not started |
| [#38](https://github.com/lambdaclass/tekton/issues/38) | Constrained approval ergonomics | Not started |
| [#36](https://github.com/lambdaclass/tekton/issues/36) | Trajectory inspection and evaluation tooling | Not started |
| [#37](https://github.com/lambdaclass/tekton/issues/37) | Deterministic task replay | Not started |
| [#40](https://github.com/lambdaclass/tekton/issues/40) | Playbooks from successful runs | Not started |
| [#32](https://github.com/lambdaclass/tekton/issues/32) | Prewarmed sandbox image pipeline | Not started |
| [#31](https://github.com/lambdaclass/tekton/issues/31) | Elastic infrastructure (multi-node scaling) | Not started |

## P2: Workflow distribution and automation (after P1)

| Issue | Title | Status |
|-------|-------|--------|
| [#42](https://github.com/lambdaclass/tekton/issues/42) | CI integration and agent self-correction | Not started |
| [#44](https://github.com/lambdaclass/tekton/issues/44) | GitHub App integration | Not started |
| [#45](https://github.com/lambdaclass/tekton/issues/45) | Workflow-native integrations (Slack, Linear, Jira) | Not started |
| [#46](https://github.com/lambdaclass/tekton/issues/46) | Unified multi-client session sync | Not started |
| [#47](https://github.com/lambdaclass/tekton/issues/47) | Agent memory (per-repo knowledge base) | Not started |
| [#48](https://github.com/lambdaclass/tekton/issues/48) | Agent tool ecosystem (MCP-style plugins) | Not started |
| [#49](https://github.com/lambdaclass/tekton/issues/49) | Built-in web research tools | Not started |
| [#50](https://github.com/lambdaclass/tekton/issues/50) | In-sandbox collaborative editor and verification surface | Not started |
| [#51](https://github.com/lambdaclass/tekton/issues/51) | Private workspaces | Not started |
| [#52](https://github.com/lambdaclass/tekton/issues/52) | API and SDK | Not started |
| [#53](https://github.com/lambdaclass/tekton/issues/53) | Custom agent runtimes | In progress (preview runtimes pluggable, agent runtimes remaining) |
| [#54](https://github.com/lambdaclass/tekton/issues/54) | Skills and prompt configuration | Not started |
| [#55](https://github.com/lambdaclass/tekton/issues/55) | Duplicate work detection | Not started |
| [#43](https://github.com/lambdaclass/tekton/issues/43) | Real-time collaboration (multi-user) | Not started |
| [#56](https://github.com/lambdaclass/tekton/issues/56) | Multi-agent tasks | Not started |
| [#57](https://github.com/lambdaclass/tekton/issues/57) | Local model support | Not started |
| [#58](https://github.com/lambdaclass/tekton/issues/58) | Sandbox network policies | Not started |

## P3: Cross-repo workflows and orchestration (after P2)

| Issue | Title | Status |
|-------|-------|--------|
| [#59](https://github.com/lambdaclass/tekton/issues/59) | Approval gates | Not started |
| [#60](https://github.com/lambdaclass/tekton/issues/60) | Rollback/undo | Not started |
| [#61](https://github.com/lambdaclass/tekton/issues/61) | Notifications and integrations | Not started |
| [#62](https://github.com/lambdaclass/tekton/issues/62) | Task templates and playbooks | Not started |
| [#63](https://github.com/lambdaclass/tekton/issues/63) | Scheduled and recurring tasks | Not started |
| [#64](https://github.com/lambdaclass/tekton/issues/64) | Event-driven tasks | Not started |
| [#65](https://github.com/lambdaclass/tekton/issues/65) | Code review as a task type | Not started |
| [#66](https://github.com/lambdaclass/tekton/issues/66) | Multi-repo tasks | Not started |
| [#67](https://github.com/lambdaclass/tekton/issues/67) | Monorepo support | Not started |
| [#68](https://github.com/lambdaclass/tekton/issues/68) | Multi-runtime support | Not started |
| [#69](https://github.com/lambdaclass/tekton/issues/69) | Image and log attachments in prompts | Not started |

## P4: Intelligence and observability (after P3)

| Issue | Title | Status |
|-------|-------|--------|
| [#70](https://github.com/lambdaclass/tekton/issues/70) | Repo onboarding and scoring | Not started |
| [#71](https://github.com/lambdaclass/tekton/issues/71) | Audit log depth and retention | Not started |
| [#72](https://github.com/lambdaclass/tekton/issues/72) | Agent quality metrics | Not started |
| [#73](https://github.com/lambdaclass/tekton/issues/73) | Adoption and impact KPIs | Not started |
| [#74](https://github.com/lambdaclass/tekton/issues/74) | Org-wide intelligence | Not started |
| [#75](https://github.com/lambdaclass/tekton/issues/75) | Task forking and checkpoints | Not started |
| [#76](https://github.com/lambdaclass/tekton/issues/76) | Merge conflict resolution | Not started |

## Backlog: Dashboard and UX

| Issue | Title | Status |
|-------|-------|--------|
| [#77](https://github.com/lambdaclass/tekton/issues/77) | In-preview console | Not started |
| [#78](https://github.com/lambdaclass/tekton/issues/78) | Per-project environment variables | Not started |
| [#85](https://github.com/lambdaclass/tekton/issues/85) | Run real-world apps in preview environments | In progress (DB, multi-service, secrets supported; needs more testing) |
