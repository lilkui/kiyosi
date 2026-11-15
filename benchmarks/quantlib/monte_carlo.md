# European and continuous-barrier Monte Carlo

The common absolute price budget is 0.10 on spot 100. Before profile selection,
the initial predefined validation seeds were 1 through 32, and timing seed is
1729. The initial ensemble exposed a structural RNG collision at seed 5 in
QuantLib barriers: the Gaussian inverse-normal stream and auxiliary crossing
uniform stream become dependent. All original results are retained in
`calibration/monte_carlo_seed_collision.json`. Before observing replacement
outputs, the whole suite adopted the deterministic validation formula
`1009 + 97*i`, `i=0..31`, excluding seed 5 and zero by construction. No seed was
selected individually by accuracy. Both native barrier protocols reject primary
seed 5 with an explicit independence diagnostic. Timing seed 1729 is unchanged.
Both backends must achieve empirical RMSE <= 0.05, absolute signed bias <= 0.025,
sample standard deviation <= 0.05, and every validation/timing absolute error
plus reference uncertainty <= 0.10. These are empirical finite-ensemble checks,
not a proof of population error bounds. A wide confidence interval cannot pass
an insufficient-precision estimator. The live runner repeats the full ensemble
before every timing run, including `--price-only`.

Path refinement uses 65,536, 131,072, and 262,144 physical paths. QuantLib barrier
time refinement uses 1, 2, 4, and 8 exact BSM evolution intervals with continuous
sampled bridge crossing correction. European BSM and Kiyosi barriers use a single
exact terminal draw; a Kiyosi European time grid is unused. Zero barrier rebate
avoids approximating the time of rebate payment.

The benchmark protocol accepts nonzero 32-bit seeds (1 through 2^32-1),
preventing aliases of QuantLib's auxiliary seed after narrowing. This restricts
the benchmark and does not change the Rust library's u64 seed domain.
Both backends use one worker, pseudorandom sampling, no antithetic pairs and no
control variate. Kiyosi uses block-stream PCG32 / Box–Muller (blocks of 256);
QuantLib uses Mersenne Twister / inverse-normal draws. Equal seeds do not imply
common paths. QuantLib's barrier pricer samples crossings using a separate MT
uniform generator hardcoded to seed 5 in QuantLib 1.44 `mcbarrierengine.hpp`;
Gaussian seed variation leaves that stream fixed. Primary seed 5 must be
avoided to keep the two streams independent. Path-count and time refinements
therefore accompany signed-bias evidence. Kiyosi integrates bridge survival
conditionally and does not sample crossing events.

With antithetic sampling disabled, `path_count` equals the number of physical
paths and estimator samples in both libraries. QuantLib's enabled-antithetic
sample count would instead count pairs; this suite rejects that policy. QuantLib
accumulates sample variance inherently while Kiyosi's ordinary `price` does not.
No variance work is subtracted. All native fresh calls regenerate all samples;
QuantLib forces recalculation and Kiyosi uses its public price API. Engine and
contract construction, reference prices, and ensemble checks are outside timing.

The four cases cover call and put European payoffs and down-and-out call /
up-and-in put continuous barriers. References reuse independently implemented
Decimal BSM closed forms and absorbing Brownian-density integrals, refined in
precision, independent of both native engines. See `generate_mc_references.py`.
Terminal digital MC, continuous-average Asian MC, double barriers and structured
products remain excluded because no directly verified counterpart is available.

`calibrate_monte_carlo.py` records every predefined seed at every path count and
barrier time refinement in `calibration/monte_carlo.json`; it does not search for
or discard favorable seeds. Normal runs use committed profiles without tuning.

Selected profiles use the smallest tested path count meeting all ensemble gates
and the unchanged timing-seed budget. Both QuantLib barrier profiles use one
step. The separate path/time refinements retain their full seed results.

| Case | Kiyosi physical paths | Kiyosi RMSE | QuantLib physical paths | QuantLib RMSE |
| --- | ---: | ---: | ---: | ---: |
| mc-european-call | 131,072 | 0.031067 | 262,144 | 0.028843 |
| mc-european-put | 65,536 | 0.026958 | 65,536 | 0.035167 |
| mc-barrier-down-out-call | 65,536 | 0.034541 | 131,072 | 0.036552 |
| mc-barrier-up-in-put | 65,536 | 0.001075 | 65,536 | 0.007316 |
