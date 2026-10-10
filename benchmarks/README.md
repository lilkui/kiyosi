# Pricing benchmark matrix

Latest report: [Windows CPU results, 2026-09-26](results/2026-09-26-windows.md), with timings, price differences, environment details, and archived raw results.

`kiyosi_benchmarks` includes one fixed Black–Scholes–Merton scenario for every supported C++ instrument and pricing engine pairing. The `matrix/kiyosi/` and `matrix/quantlib/` names share a suffix when both libraries price the same contract. `matrix/kiyosi_only/` marks a pairing without a direct QuantLib C++ engine. The optional CUDA backend adds seven more `kiyosi_only` cases when Kiyosi is built with CUDA.

## Build and run

Install QuantLib C++ with its CMake package, then configure a Release build. CMake adds QuantLib comparisons automatically when it finds the package. QuantLib 1.43 matches the version used by the repository's reference tooling.

On Windows with vcpkg `QuantLib:x64-windows-static`, run these commands from the repository root in a Visual Studio Developer PowerShell. They build and run every registered CPU benchmark, including the QuantLib comparisons. CUDA benchmarks require a separate CUDA-enabled build.

```powershell
$env:VCPKG_ROOT = 'C:/path/to/vcpkg'
cmake --preset windows-release --fresh -DKIYOSI_BUILD_BENCHMARKS=ON -DBUILD_TESTING=OFF -DKIYOSI_BUILD_EXAMPLES=OFF -DCMAKE_MSVC_RUNTIME_LIBRARY=MultiThreaded -DCMAKE_TOOLCHAIN_FILE="${env:VCPKG_ROOT}/scripts/buildsystems/vcpkg.cmake" -DVCPKG_TARGET_TRIPLET=x64-windows-static
cmake --build --preset windows-release --target kiyosi_benchmarks
./out/build/windows-release/benchmarks/kiyosi_benchmarks.exe --benchmark_repetitions=5 --benchmark_report_aggregates_only=true --benchmark_out=out/build/windows-release/benchmarks.json --benchmark_out_format=json
python benchmarks/compare.py out/build/windows-release/benchmarks.json
```

For a manually installed QuantLib package, replace the toolchain and triplet arguments with `-DQuantLib_DIR=C:/path/to/QuantLib/lib/cmake/QuantLib`. Add `--benchmark_filter=^matrix/` to the benchmark command for a matrix-only run.

On Linux, use `linux-release` and `./out/build/linux-release/benchmarks/kiyosi_benchmarks`, omitting the Windows-specific CMake options. Without a QuantLib C++ package, CMake builds the Kiyosi cases alone. `compare.py` accepts either aggregate or individual Google Benchmark JSON results and prints median times, `QuantLib / Kiyosi` ratios, and warmup price differences. A ratio above 1 means Kiyosi took less CPU time in a paired case.

## Scenario and interpretation

The `analytics/kiyosi/` cases measure accumulator and snowball finite-difference
delta/gamma requests beside price-only baselines using the same contracts and
default 200-by-200 grids. They value at noon on 2025-01-01 to avoid a current
observation discontinuity. A European Monte Carlo implied-volatility case uses
5,000 paths, seed 42, and the default solver settings; the input quote is prepared
before timing. The `volatility` counter records the solved value. Each timed
Greek request retains the complete result so its sensitivities cannot be optimized
away. Run these cases with `--benchmark_filter=^analytics/`; `compare.py` continues
to report only the pricing matrix.

- Common market: valuation on 2025-01-01, expiry on 2026-01-01, spot and vanilla strike 100, flat risk-free rate 4%, dividend yield 1%, volatility 20%, Actual/365 Fixed.
- Instruments and engines are constructed before timing. Each iteration calls Kiyosi `price()` or forces a QuantLib `recalculate()` followed by `NPV()`; plain repeated `NPV()` would report a cached lookup. The warmup price is included as a `price` counter for checking that paired cases value similar contracts.
- CPU cases report CPU time; CUDA cases report wall time, which includes GPU synchronization inside the pricing call.
- CRR trees use 128 steps. Finite difference cases use 80 time and 80 asset steps. Vanilla Monte Carlo uses 5,000 Kiyosi paths and 2,500 antithetic QuantLib sample pairs, with 10 European or 20 American steps and seed 42. Autocallable Monte Carlo uses 2,000 paths and seed 42.
- The 23 QuantLib pairs cover European and American vanilla, cash and asset digital, vanilla and binary barriers, cash and asset one-touch, and continuous geometric and arithmetic Asian contracts. Ten accumulator and autocallable pairings have no direct QuantLib engine. Digital quadrature uses QuantLib `IntegralEngine`; one-touch uses the sum of two QuantLib binary barrier contracts.
- Results compare these configured implementations, not equal amounts of internal work. Kiyosi `price()` and QuantLib `NPV()` both request price only. The Bjerksund–Stensland engines use different published approximations (Kiyosi 2002, QuantLib 1993); finite difference grids, integration methods, and Monte Carlo regression also differ. Inspect price differences before interpreting speed ratios.

Run on an otherwise idle machine, use the same compiler optimization level for both libraries, and keep the QuantLib version and CPU/GPU details with published results.
