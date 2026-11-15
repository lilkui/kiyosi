# Supplied local-volatility finite differences

`cases/fd_local_vol.json` supplies eleven counterparts to the BSM FD slice:
European call and American put, cash and asset digitals, single barrier,
cash/asset terminal binary barriers, cash/asset no-touch at expiry, and cash/asset
one-touch at hit. All contracts begin at valuation. Barriers use continuous
monitoring and the verified native mappings in `fd_bsm.md`. Each case has the same
absolute price-error budget of **0.005** for both backends, declared before
profile calibration. This is a benchmark target, independent of the broader
Python regression tolerances.

The model is a supplied local-volatility surface, with dates 2025-01-01 and
2026-01-01, spots 80 and 120, and time rows `[.30,.15]`, `[.35,.20]`.
Interpolation is linear in Actual/365 Fixed time and spot. Spots outside the
surface use the nearest spot endpoint; dates must remain within coverage.
The scalar `surface_*` inputs in every scenario are the actual construction
parameters. QuantLib transposes the time-row table into its spot-row matrix,
uses `FixedLocalVolSurface` in the process's external local-volatility handle,
and sets the native engine's `localVol=true`. No implied-volatility or Dupire
conversion enters this model.

QuantLib's Black constant volatility, explicitly recorded as `mesher_vol`, sizes
its automatic native log-space mesher only. It does not supply the local
volatility in the differential operator. Kiyosi uses uniform asset intervals
and an explicit upper boundary; QuantLib uses its public engine's automatic
mesher. Count and domain equality are not claimed. Native schemes, damping,
counts, upper boundary, and mesher controls are in the backend profiles.
QuantLib's inherent sensitivity calculations remain part of fresh price work.

`references/fd_local_vol.json` contains actual QuantLib 1.44 prices from the
offline reference driver. The generator refines time through 3,200, 6,400,
12,800 steps at 3,200 space nodes, compares consecutive first-order time
extrapolates, and separately doubles space to 6,400 nodes. The asset digital
uses 25,600/51,200 space nodes to resolve its larger terminal discontinuity,
with separate 800/1,600/3,200 time refinements. The asset terminal binary
barrier uses 25,600/51,200 space nodes and the same independent time sequence,
with QuantLib's built-in implicit Euler reference scheme. Fully implicit steps
avoid the terminal-jump oscillation observed when Crank–Nicolson with two
damping steps is used on these fine spatial grids with too few time steps.
The timed binary-barrier profile still uses Crank–Nicolson. The generator
independently varies the mesher-only Black volatility through .4/.5/.6 at that fine grid,
changing automatic domain size without changing the supplied local model.
The reference adds the measured spatial correction to the fine time
extrapolate. Its uncertainty estimate is twice the sum of the observed time,
space, and mesher-domain shifts, plus `1e-10` for numerical roundoff, and must
be at most 10% of the shared case budget. These empirical convergence estimates
are not rigorous error bounds. Rejected coarse/underdamped refinements are
retained in `calibration/local_vol_rejected_references.json`, including every
actual input/profile needed to replay its native reference-driver request.

`calibration/fd_local_vol.json` records each actual selected-profile price and
its error against that independently refined reference. Time and space are also
doubled separately for each backend. Kiyosi enlarges its upper domain from 400
to 600 at constant spacing, while QuantLib changes its mesher volatility
independently. Coarse 10/10 profiles demonstrate insufficient accuracy.
The asset digital also has an independent Rust reference cross-check: 6,400
asset intervals at 1,600/3,200/6,400 time steps, followed by 12,800 intervals and
a constant-spacing upper-domain enlargement to 600. These agree inside the
QuantLib reference uncertainty. The asset binary also checks Rust time
800/1,600/3,200 at 3,200 intervals, then space 6,400 and a constant-spacing
upper-domain enlargement to 600. Representative surface evaluations include
interior spots and both endpoint extrapolation regions, and agree to `1e-14`. Surface validation and reference
generation are outside timed price requests.

After building the companions, reproduce the evidence without downloading or
building QuantLib:

```sh
python3 benchmarks/quantlib/generate_local_vol_references.py
python3 benchmarks/quantlib/calibrate_local_vol.py
KIYOSI_QUANTLIB_LIVE=1 python3 -m unittest discover -s benchmarks/quantlib -p test_local_vol.py
python3 benchmarks/quantlib/run.py --no-build --cases benchmarks/quantlib/cases/fd_local_vol.json
```

Ordinary runs read committed references; they do no reference refinement.
Development timing can overlap other numerical calibration processes. The final
full-suite run provides the definitive timing evidence on a quiet machine.
