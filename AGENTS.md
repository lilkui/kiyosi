# AGENTS.md

## Development

This project is in Alpha with no legacy constraints; decisive refactoring for clarity is encouraged, and backward compatibility or API/ABI stability is unnecessary.

## Build

Activate the Visual Studio Developer environment before using the MSVC or CMake toolchains.

## Commits

Write commit messages adhering to the Conventional Commits specification.

## Agent skills

### Issue tracker

Issues live as markdown files under `.scratch/<feature-slug>/`. See `docs/agents/issue-tracker.md`.

### Triage labels

Uses the default canonical labels: `needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, and `wontfix`. See `docs/agents/triage-labels.md`.

### Domain docs

Uses a single-context layout with root `CONTEXT.md` and `docs/adr/`. See `docs/agents/domain.md`.
