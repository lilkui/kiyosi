# Native QuantLib Benchmarks

Native latency and accuracy benchmark suite comparing Kiyosi (Rust) against QuantLib C++ 1.44 across 59 scenarios, 51 required semantic cells, and 20 eligible engines.

Both backends must satisfy shared accuracy budgets (`abs(price - reference) + reference_uncertainty <= error_budget`) before latency ratios (`Q/K`, where `> 1` means lower Kiyosi latency) are reported. Contract, context, model, and engine construction happen outside the timed loop.

## Prerequisites

- Linux
- Rust / Cargo
- Python 3.12+ (standard library only for normal runs)
- C++17 compiler and Boost headers
- **QuantLib C++ 1.44** compiled with release optimizations (`-O3 -DNDEBUG`), discoverable via `quantlib-config` and the runtime linker (`LD_LIBRARY_PATH`)

> Cargo-only library builds, tests, docs, and packaging do not require QuantLib.

## Quick Start

From the repository root:

```sh
# Build native binaries, verify accuracy, run timing batches, and write benchmarks/quantlib/report.json
python3 benchmarks/quantlib/run.py

# Use a nonstandard QuantLib installation prefix
QUANTLIB_CONFIG=/opt/quantlib-1.44/bin/quantlib-config \
LD_LIBRARY_PATH=/opt/quantlib-1.44/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH} \
python3 benchmarks/quantlib/run.py
```

## Common Commands

```sh
# Run specific case(s) and write report to a custom path
python3 benchmarks/quantlib/run.py --case european-bsm-atm-call --report /tmp/bsm.json

# Check accuracy only using existing binaries (skip build and timing)
python3 benchmarks/quantlib/run.py --no-build --price-only

# Run the benchmark suite unit tests
python3 -m unittest discover -s benchmarks/quantlib -p 'test_*.py'
```

### Key CLI Flags (`run.py`)

| Flag | Description |
| :--- | :--- |
| `--case <id>` | Run specific scenario ID(s) (repeatable). Without `--case`, all 51 required semantic cells must be covered. |
| `--no-build` | Reuse existing Rust (`target/release/examples/quantlib_bench`) and C++ (`benchmarks/quantlib/build/quantlib_bench`) binaries. |
| `--price-only` | Verify accuracy gates without collecting timing samples. |
| `--target-ms <ms>` | Minimum duration per adaptive timing batch (default: `20`). |
| `--report <path>` | Output JSON report path (default: `benchmarks/quantlib/report.json`). |
| `--quantlib-config`, `--cxx`, `--cxxflags` | Override build discovery and compiler flags (or set `QUANTLIB_CONFIG`, `CXX`, `CXXFLAGS`). |

## Architecture & Offline Calibration

- `cases/*.json` — Scenario inputs, error budgets, and per-backend numerical work profiles.
- `references/*.json` — Independent reference values, uncertainties, and provenance.
- `calibration/*.json` — Offline convergence and calibration evidence.
- `crates/kiyosi/examples/quantlib_bench.rs` & `benchmarks/quantlib/quantlib.cpp` — Native companion executables (`--info` or `key=value` args).

Reference generation and profile calibration are offline steps (`generate_*_references.py` and `calibrate_*.py`). Build the native companions first, then run the relevant script when adding or modifying scenarios:

```sh
python3 benchmarks/quantlib/run.py --price-only --case european-bsm-atm-call
python3 benchmarks/quantlib/generate_european_references.py
python3 benchmarks/quantlib/calibrate_numerical.py
```

## Family & Coverage Documentation

- [Coverage and exclusion inventory](coverage.md)
- [Digitals and continuous Asians](digitals_asian.md)
- [Analytic barriers and touch options](barriers.md)
- [European and barrier Monte Carlo](monte_carlo.md)
- [American least-squares Monte Carlo](american_monte_carlo.md)
- [BSM finite difference](fd_bsm.md)
- [Heston analytic and finite difference](heston.md)
- [Local volatility finite difference](fd_local_vol.md)
