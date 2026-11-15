# Analytic continuous barrier and touch cases

`cases/barriers.json` covers four Kiyosi engines and all their directly matched
product families under flat-rate BSM. Every instrument starts at valuation,
strictly inside its continuous barriers, with one Actual/365 Fixed year to expiry.
The shared absolute price budget is `1e-9`; reference uncertainty is `1e-12`.
Cash and asset payoffs are represented, without an exhaustive direction/payoff
cross-product. Default timing uses the coordinator's 15 alternating pairs.

| Family | Native QuantLib contract and engine | Settlement |
| --- | --- | --- |
| Single vanilla barrier | `BarrierOption`, `AnalyticBarrierEngine` | KO rebate at hit; KI rebate on survival at expiry |
| Terminal cash/asset binary barrier | `BarrierOption`, `AnalyticBinaryBarrierEngine` | `AmericanExercise(..., payoffAtExpiry=true)`; terminal strike condition retained |
| Single touch cash/asset | `VanillaOption`, `AnalyticDigitalAmericanEngine` | Native payoff strike equals barrier; exercise flag selects hit/expiry payment |
| Single no-touch cash/asset | `VanillaOption`, `AnalyticDigitalAmericanKOEngine` | Payment at expiry on survival; payoff side follows barrier direction |
| Double vanilla barrier | `DoubleBarrierOption`, `AnalyticDoubleBarrierEngine` | Zero rebate; strike strictly between barriers |
| Double cash touch/no-touch | `DoubleBarrierOption`, `AnalyticDoubleBarrierBinaryEngine` | European exercise; unconditional cash at expiry on activation/survival |

For single asset-at-hit, continuity gives `S_tau=H`, so a native cash binary
paying exactly `H` represents that contract. This uses one native instrument and
engine. A terminal binary retains an additional terminal strike condition;
it is a separate family from an unconditional touch payment.

Scheduled monitoring/corrections, prior touches, double-barrier nonzero rebates,
strikes outside double barriers, mixed KIKO/KOKI activation, asset double-touch,
and double-touch at-hit payments are excluded. No custom QuantLib pricing engine
or portfolio decomposition is used. The native parsers reject extra parameters;
they do not quietly accept unsupported monitoring or settlement controls.

## Independent reference evaluation

Run `python3 benchmarks/quantlib/generate_barrier_references.py` to regenerate
`references/barriers.json` using only the Python standard library. No backend
price enters this calculation. The generator integrates the absorbing Brownian
log-price transition density against the contractual payoff. Let
`X=log(S)`, `mu=r-q-sigma²/2`, variance `v=sigma²T`. A free normal density with
mean `m` has truncated moment

```text
M_beta(m,a,b) = exp(beta*m + beta²*v/2)
               [Phi((b-m-beta*v)/sqrt(v)) - Phi((a-m-beta*v)/sqrt(v))].
```

For one absorbing boundary `h=log(H)`, subtract the reflected normal centered at
`2h-X+mu*T`, weighted by `exp(2mu(h-X)/sigma²)`, and integrate only on the surviving
side of the boundary. Cash uses order zero and asset uses order one. Vanilla
payoffs combine those moments over the strike interval. Knock-in values use the
unrestricted terminal expectation minus the surviving expectation. Expiry
payments are discounted by `exp(-rT)`.

For two boundaries `l,u`, width `w=u-l`, sum the image pairs centered at
`X-2nw+mu*T` and `2l-X-2nw+mu*T`, with weights
`exp(mu*(m-X)/sigma²)` where `m` denotes the center before adding drift. Integrate
only over `[l,u]` intersected with the terminal payoff interval. The committed
reference uses `n=-12,...,12`, compared with `n=-8,...,8`.

At-hit cash payments use the finite-horizon Laplace transform of the Brownian
first-passage density. For distance `a=abs(log(H/S))`, drift toward the boundary
`nu`, and `lambda=sqrt(nu²+2r sigma²)`, it is

```text
exp((nu-lambda)*a/sigma²) Phi((lambda*T-a)/(sigma*sqrt(T)))
+ exp((nu+lambda)*a/sigma²) Phi((-lambda*T-a)/(sigma*sqrt(T))).
```

Normal probabilities use an independently evaluated integrated exponential
Taylor series with Decimal arithmetic at 90 and 110 digits. The tail cutoff at
16 standard deviations has Mills bound `6.4e-58` before image weights. On these
fixed scenarios, the weights and cash/asset scales leave this far below `1e-12`;
omitted images decay quadratically in the image index. The retained evidence
records both precision and image refinements. Maximum precision difference is
`1.2e-36`; 8-to-12 image refinement changes no represented digit at precision 110.
The declared `1e-12` uncertainty also comfortably covers conversion to binary64.
This is scenario-specific evidence, not a claim for arbitrary parameters.

## Native calibration and semantic evidence

After building, run `python3 benchmarks/quantlib/calibrate_barriers.py` to record
`calibration/barriers.json`. With installed QuantLib 1.44, every native price
passes the shared budget. Maximum observed absolute error is below `5e-14`.
QuantLib double-vanilla series counts 3, 5, 10 and 20 give the same binary64 prices;
5 is the explicit benchmark profile. Kiyosi has no public series setting:
its double engines select absorbing image/spectral series internally (limits
64 image / 256 spectral terms with convergence checks). The double binary
QuantLib engine exposes no convergence setting and uses 99 Hui terms, requiring
the last term to be below `1e-8` internally; the independent case gate still
requires actual price error below `1e-9`.
These are convergent analytic formulations with different arithmetic and costs.

`python3 -m unittest benchmarks.quantlib.test_barriers` builds both real native
adapters, checks every price, and checks all 15 paired timing samples per case.
It also checks cash/asset touch plus no-touch identities, double-touch plus
no-touch discounting, asset-at-hit equals barrier cash-at-hit, and the payment
premium from earlier cash settlement. These complement the independent prices;
parity alone does not establish numerical accuracy.

Timing measures fresh public requests with objects already constructed. QuantLib
`recalculate()` plus `NPV()` includes its engine's inherent extra sensitivities
(the American digital engine computes delta, gamma and rho for at-hit payment;
its expiry branch and the double binary engine compute value only). Rust calls public `price` through
compiler-resistant inputs/results. No extra work is subtracted from either side.

The payment flags and extra-work statements follow the actual QuantLib 1.44
[American digital source](https://github.com/lballabio/QuantLib/blob/v1.44/ql/pricingengines/vanilla/analyticdigitalamericanengine.cpp).
The fixed Hui loop and value-only path follow the
[double binary source](https://github.com/lballabio/QuantLib/blob/v1.44/ql/pricingengines/barrier/analyticdoublebarrierbinaryengine.cpp);
its header's numerical-Greek description does not describe this executed path.
