# Delivery roadmap

Product invariants and engineering requirements live in `AGENTS.md`.
Feature status is tracked in [FEATURE-ROADMAP.md](FEATURE-ROADMAP.md).
The [current implementation boundaries](CURRENT-STATE.md) describe the
Agent/Editor split and native authority model.

Delivery requires independently running formatting, lint, unit tests, budget
tests, the desktop web build, bundle verification, and policy checks. Native
changes also require Rust formatting, Clippy, and relevant native tests.
Rendered resource changes require the applicable browser geometry and visual
checks, with intentional baseline changes explicitly reported.

The retained work-item contracts under `docs/work-items/` define bounded
ACP runtime and acceptance requirements. These documents are kept in the
public checkout because the ownership manifest includes them; gate evidence
belongs outside tracked source files.
