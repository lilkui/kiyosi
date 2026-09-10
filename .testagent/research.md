# Numerical European references

Scope: issue 03, five European numerical engines and the existing fixture consumer.
Public seams authorized by the issue: engine `price`, `NumericalAnalyticsEngine::price`,
and frozen fixture generation. Existing Catch2 assertions compare independent values.

Acceptance checklist:
- Replace all five matching legacy prices and three convergence targets; retain other rows.
- Reconstruct every generated market, contract and engine setting; reject unknown dispatch.
- Compare native promised measures across call/put, 80/100/120 spot, 30/365/730 days.
- Compare all ten wrapper Greeks on smooth representative calls and puts.
- Keep reference uncertainty separate from per-engine absolute budgets and MC metadata.
- Reject unstable references, document rationale, regenerate identically, run C++ alone.

Inventory: `generate.py`, `scenarios.json`, `check_generation.py`, fixture TSV,
`fixture_tests.cpp`; production engines inspected under `src/pricing/engines/vanilla`.
Tree/FD native contracts: price/delta/gamma. Integral/MC: price. Wrapper differentiates
prices. FD supports a fixed upper boundary; MC uses antithetic exact GBM increments.
