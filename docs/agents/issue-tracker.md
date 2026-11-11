# Issue tracker: Local Markdown

Issues and specs for this repo live as Markdown files in `.scratch/`.

## Conventions

- One feature per directory: `.scratch/<feature-slug>/`.
- The spec is `.scratch/<feature-slug>/spec.md`.
- Implementation issues are individual files at `.scratch/<feature-slug>/issues/<NN>-<slug>.md`, numbered from `01`.
- Record triage state in a `Status:` line near the top of each issue (see `triage-labels.md`).
- Append comments and conversation history under a `## Comments` heading.

## Publish and fetch

When a skill says to publish, create a file under `.scratch/<feature-slug>/`, creating the directory if needed.

When a skill says to fetch a ticket, read the referenced file. The user will normally provide its path or issue number.

## Wayfinding operations

- Map: `.scratch/<effort>/map.md`, containing notes, decisions so far and fog.
- Child tickets: `.scratch/<effort>/issues/NN-<slug>.md`, numbered from `01`, with the question in the body. Use a `Type:` line for `research`, `prototype`, `grilling` or `task`, and a `Status:` line for `claimed` or `resolved`.
- Blocking: list dependencies near the top as `Blocked by: NN, NN`. A ticket is unblocked when all listed tickets are resolved.
- Frontier: scan for open, unblocked and unclaimed tickets; the first by number wins.
- Claim: set `Status: claimed` and save before starting work.
- Resolve: append the answer under `## Answer`, set `Status: resolved`, and append a context pointer to the map's decisions.
