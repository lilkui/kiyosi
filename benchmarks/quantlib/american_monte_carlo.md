# American least-squares Monte Carlo

The ATM put uses S=K=100, r=.05, q=.02, sigma=.2 and one year. Its refined
American FD reference is 6.660684545893456 with numerical uncertainty .00005
(calibration/american.json), versus European value 6.330080627549918. This
premium requires continuation fitting and exercise decisions.

Before collecting LSM outputs, the common maximum absolute error budget is .15,
ensemble RMSE limit .075, absolute signed mean error limit .05 and empirical
sample standard deviation limit .075. The 32 validation seeds are 1009+97*i,
i=0..31; timing seed 1729 is separate. QuantLib calibration seeds add 10000.
No seed is selected by observed accuracy. Equal seeds do not imply equal paths.

Kiyosi fits continuation on in-the-money rows of the pricing simulation itself,
with basis `1, S/K, (S/K)^2` and lower-degree fallback. It stores exercise paths;
there is no additional calibration sample or supported American statistics API.
QuantLib uses independent calibration/pricing simulations, explicit quadratic
monomial order 2 and its built-in additional payoff basis. Antithetic sampling
and control variates are disabled for both calibration and pricing. One worker
is enforced. QuantLib path construction uses no Brownian bridge.

Each call simulates and fits afresh. QuantLib recalculate() invokes
MCLongstaffSchwartzEngine::calculate(), which constructs its path pricer and
calibrates before pricing. Nothing caches continuation coefficients across
timed calls. QuantLib also accumulates pricing sample variance and exercise probability internally.

Run calibrate_american_monte_carlo.py after building native companions to retain
all seeds at 65536/131072 pricing paths, 25/50/100 exercise intervals, and
32768/65536 QuantLib calibration paths. Its paired refinement mean differences
and empirical mean standard errors describe finite-ensemble uncertainty;
ordinary path SE does not bound exercise or regression bias. The live runner
repeats selected-profile ensembles and checks errors, bias and dispersion before
the 15 paired timing samples. Calibration evidence is outside timing.

The selected profiles use 65536 pricing paths and 50 exercise intervals. QuantLib
additionally uses 32768 calibration paths per price; Kiyosi trains on its pricing
paths. Both keep the original timing seed. Across the 32 validation seeds:

| Backend | RMSE | Signed mean error | Empirical mean SE | Sample SD | Maximum absolute error |
| --- | ---: | ---: | ---: | ---: | ---: |
| Kiyosi | .04052 | -.02415 | .00584 | .03306 | .07952 |
| QuantLib | .04017 | -.02545 | .00558 | .03157 | .09566 |

The negative signed errors retain the residual continuation/exercise bias in the
accuracy evidence. The empirical mean SE measures variation across independent
seed prices; the reference's separate .00005 numerical uncertainty is included
in the live accuracy gates. Full seed values and refinement differences are in
`calibration/american_monte_carlo.json`.

All nine refinement ensembles satisfy the same declared limits. Doubling pricing
paths reduces RMSE to .03323 (Kiyosi) and .03152 (QuantLib). At 131072 pricing
paths, changing 50 to 100 exercise intervals changes the seed-mean price by
-.00141 (empirical mean SE .00340) and +.00075 (SE .00297), respectively.
Doubling QuantLib calibration paths from 32768 to 65536 changes its mean by
+.000046 (SE .000716). The selected 50-interval, 32768-calibration profile keeps
the remaining exercise/regression error inside the common bias allowance.

QuantLib source: [AmericanPathPricer basis](https://github.com/lballabio/QuantLib/blob/v1.44/ql/pricingengines/vanilla/mcamericanengine.cpp) and [per-call calibration](https://github.com/lballabio/QuantLib/blob/v1.44/ql/pricingengines/mclongstaffschwartzengine.hpp).
