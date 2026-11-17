# Native QuantLib benchmarks

Compare Kiyosi Rust and QuantLib C++ latency after both pass the same accuracy
gates. See [coverage and exclusions](coverage.md) for the supported subset and
[methodology](methodology.md) for reference and algorithm details.

## Requirements

- Linux, Rust/Cargo, Python 3.12+ (standard library only).
- C++17 compiler and Boost headers.
- QuantLib C++ **1.44**, built with `-O3 -DNDEBUG`, discoverable through
  `quantlib-config` and the runtime linker. The Python QuantLib package alone
  is insufficient. Cargo-only library workflows do not require QuantLib.

## Run

From the repository root:

```sh
# Build both companions, check accuracy, time all cases, and write report.json.
python3 benchmarks/quantlib/run.py

# Focus on one case; repeat --case to select more.
python3 benchmarks/quantlib/run.py --case european-bsm-atm-call --report /tmp/bsm.json

# Check accuracy with existing binaries, without timing.
python3 benchmarks/quantlib/run.py --no-build --price-only
```

For a nonstandard QuantLib installation:

```sh
QUANTLIB_CONFIG=/opt/quantlib-1.44/bin/quantlib-config \
LD_LIBRARY_PATH=/opt/quantlib-1.44/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH} \
python3 benchmarks/quantlib/run.py
```

| Option | Purpose |
| --- | --- |
| `--case ID` | Select a case; repeatable. Without this option, all required coverage cells must be present, even with a custom manifest. |
| `--cases PATH`, `--references PATH` | Load a JSON file or directory; defaults to `cases/` and `references/` beside the runner. Use `--case` for a partial manifest. |
| `--no-build` | Reuse `target/release/examples/quantlib_bench` and `benchmarks/quantlib/build/quantlib_bench`. |
| `--price-only` | Run accuracy and Monte Carlo ensemble gates without timing. |
| `--target-ms MS` | Minimum adaptive timing batch duration; default `20`. |
| `--report PATH` | JSON destination; default `benchmarks/quantlib/report.json`. |
| `--quantlib-config`, `--cxx`, `--cxxflags` | Build overrides; also available through `QUANTLIB_CONFIG`, `CXX`, `CXXFLAGS`. |

Run `python3 benchmarks/quantlib/run.py --help` for all options.

## Read results

Both prices must satisfy `abs(price - reference) + reference_uncertainty <= error_budget`.
Monte Carlo also checks empirical error, bias and dispersion across validation
seeds. Failed gates prevent timing; unsuccessful rows have a null ratio and make
the command exit unsuccessfully. Other requested cases continue after per-case
failures.

Timing uses 15 alternating backend pairs on preconstructed contracts, contexts,
models and engines. Each request recalculates a fresh price; inherent extra
work, including QuantLib sensitivities and sample variance, stays timed.
The ratio is `Q/K`: values above one mean lower Kiyosi latency. The JSON retains
raw samples, profiles, reference evidence, coverage and machine/build metadata.
Use an idle machine; results apply to the recorded scenarios and profiles.

## Tests and maintenance

Coverage/report checks need no QuantLib installation:

```sh
python3 -m unittest discover -s benchmarks/quantlib -p test_coverage.py
```

For the full suite, build both companions first. Several tests require native
binaries even without `KIYOSI_QUANTLIB_LIVE`; that flag enables additional
QuantLib checks:

```sh
python3 benchmarks/quantlib/run.py --price-only --case european-bsm-atm-call
KIYOSI_QUANTLIB_LIVE=1 python3 -m unittest discover -s benchmarks/quantlib -p 'test_*.py'
```

Scenario inputs, budgets and backend profiles live in `cases/*.json`;
reference values, uncertainty and provenance in `references/*.json`; offline
convergence evidence in `calibration/*.json` and `heston_calibration.json`.
Native adapters are [quantlib.cpp](quantlib.cpp) and
[quantlib_bench.rs](../../crates/kiyosi/examples/quantlib_bench.rs).
For scenario changes, follow the [reference and calibration workflow](methodology.md#maintenance).
