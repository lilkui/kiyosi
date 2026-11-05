"""Heston pricing, numerical boundaries and Python workflow contracts."""

import inspect
import math
import unittest
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timedelta

import QuantLib as ql
from kiyosi import (
    ComputationFailureError,
    InvalidInputError,
    NonConvergenceError,
    UnsupportedOperationError,
    pricing,
)
from kiyosi.instruments import EuropeanOption
from kiyosi.market import (
    BlackScholesMertonParameters,
    HestonParameters,
    PricingContext,
    RateTermStructure,
)
from kiyosi.pricing import AnalyticEuropeanEngine, AnalyticHestonEuropeanEngine

START = datetime(2025, 1, 1)
END = datetime(2026, 1, 1)
PARAMETERS = {
    "risk_free_rate": 0.05,
    "dividend_yield": 0.02,
    "initial_variance": 0.04,
    "long_run_variance": 0.04,
    "mean_reversion_speed": 1.5,
    "vol_of_vol": 0.3,
    "correlation": -0.7,
}


def model(**changes):
    return HestonParameters(**(PARAMETERS | changes))


def context(parameters=None, **changes):
    return PricingContext(
        **(
            {
                "model_parameters": parameters or model(),
                "spot_price": 100,
                "valuation_time": START,
            }
            | changes
        )
    )


def option(**changes):
    return EuropeanOption(
        **(
            {
                "option_type": "call",
                "strike": 100,
                "effective_time": START,
                "expiry_time": END,
            }
            | changes
        )
    )


def reference(parameters, instrument, tolerance, *, exp_sinh=False):
    """Independent QuantLib 1.43 Gatheral reference with Lobatto or exp-sinh."""
    assert ql.__version__ == "1.43"
    qstart = ql.Date(START.day, START.month, START.year)
    end = instrument.expiry_time
    with ql.SavedSettings():
        ql.Settings.instance().evaluationDate = qstart
        risk = ql.YieldTermStructureHandle(
            ql.FlatForward(qstart, parameters.flat_risk_free_rate, ql.Actual365Fixed())
        )
        dividend = ql.YieldTermStructureHandle(
            ql.FlatForward(qstart, parameters.flat_dividend_yield, ql.Actual365Fixed())
        )
        process = ql.HestonProcess(
            risk,
            dividend,
            ql.QuoteHandle(ql.SimpleQuote(100)),
            parameters.initial_variance,
            parameters.mean_reversion_speed,
            parameters.long_run_variance,
            parameters.vol_of_vol,
            parameters.correlation,
        )
        contract = ql.VanillaOption(
            ql.PlainVanillaPayoff(
                ql.Option.Call if instrument.option_type == "call" else ql.Option.Put,
                instrument.strike,
            ),
            ql.EuropeanExercise(ql.Date(end.day, end.month, end.year)),
        )
        qmodel = ql.HestonModel(process)
        if exp_sinh or abs(parameters.correlation) == 1:
            engine = ql.AnalyticHestonEngine(
                qmodel,
                ql.AnalyticHestonEngine.Gatheral,
                ql.AnalyticHestonEngine_Integration.expSinh(tolerance),
            )
        else:
            engine = ql.AnalyticHestonEngine(qmodel, tolerance, 100_000)
        contract.setPricingEngine(engine)
        return contract.NPV()


class HestonTests(unittest.TestCase):
    def test_subnormal_reversion_preserves_long_run_variance_growth(self):
        kappa, theta, v0 = math.ulp(0.0), 1e308, 1e-20
        parameters = model(
            risk_free_rate=0,
            dividend_yield=0,
            initial_variance=v0,
            long_run_variance=theta,
            mean_reversion_speed=kappa,
            vol_of_vol=0,
            correlation=0,
        )
        variance = v0 + (kappa * theta) / 2
        expected = 100 * math.erf(math.sqrt(variance) / (2 * math.sqrt(2)))
        self.assertAlmostEqual(expected, 6.270413576726258e-7, delta=1e-20)
        for kind in ("call", "put"):
            with self.subTest(kind=kind):
                actual = AnalyticHestonEuropeanEngine().price(
                    option(option_type=kind), context(parameters)
                )
                self.assertAlmostEqual(actual, expected, delta=expected * 1e-10)

    def test_reversion_growth_avoids_intermediate_product_overflow(self):
        parameters = model(
            risk_free_rate=0,
            dividend_yield=0,
            initial_variance=0,
            long_run_variance=1e308,
            mean_reversion_speed=1e10,
            vol_of_vol=0,
            correlation=0,
        )
        snapshot = context(
            parameters,
            valuation_time=datetime(2025, 12, 31, 23, 59, 59, 999999),
        )
        for kind in ("call", "put"):
            with self.subTest(kind=kind):
                self.assertEqual(
                    AnalyticHestonEuropeanEngine().price(
                        option(option_type=kind), snapshot
                    ),
                    100,
                )

    def test_absorbing_variance_preserves_small_discounted_payoffs(self):
        engine = AnalyticHestonEuropeanEngine(
            absolute_tolerance=0, relative_tolerance=1e-8, max_evaluations=1
        )
        for kind, risk, dividend in (("call", 1e-18, 0), ("put", 0, 1e-18)):
            with self.subTest(kind=kind):
                parameters = model(
                    risk_free_rate=risk,
                    dividend_yield=dividend,
                    initial_variance=0,
                    long_run_variance=0,
                )
                actual = engine.price(
                    option(option_type=kind, strike=1e18),
                    context(parameters, spot_price=1e18),
                )
                self.assertAlmostEqual(actual, 1.0, delta=1e-12)

    def test_rare_large_variance_does_not_hide_the_fourier_tail(self):
        parameters = model(
            risk_free_rate=0,
            dividend_yield=0,
            initial_variance=0.04,
            long_run_variance=1e40,
            mean_reversion_speed=1,
            vol_of_vol=1e40,
            correlation=0,
        )
        expected = reference(parameters, option(), 1e-12, exp_sinh=True)
        self.assertAlmostEqual(expected, 59.02117958365989, delta=1e-12)
        self.assertAlmostEqual(
            reference(parameters, option(), 1e-10, exp_sinh=True), expected, delta=1e-12
        )
        for tolerance, budget in ((1e-8, 10_000), (1e-12, 100_000)):
            with self.subTest(tolerance=tolerance):
                actual = AnalyticHestonEuropeanEngine(
                    absolute_tolerance=tolerance,
                    relative_tolerance=tolerance,
                    max_evaluations=budget,
                ).price(option(), context(parameters))
                self.assertAlmostEqual(
                    actual, expected, delta=tolerance + tolerance * expected
                )

    def test_prices_match_converged_quantlib_references(self):
        engine = AnalyticHestonEuropeanEngine()
        refined = AnalyticHestonEuropeanEngine(
            absolute_tolerance=1e-11, relative_tolerance=1e-11, max_evaluations=40_000
        )
        for parameters in (
            model(),
            model(mean_reversion_speed=0.2, vol_of_vol=1.0, correlation=-0.9),
            model(correlation=0.8, vol_of_vol=0.8),
        ):
            for days in (7, 365, 3650):
                for strike in (80, 100, 120):
                    for kind in ("call", "put"):
                        contract = option(
                            option_type=kind,
                            strike=strike,
                            expiry_time=START + timedelta(days=days),
                        )
                        with self.subTest(
                            days=days,
                            strike=strike,
                            kind=kind,
                            correlation=parameters.correlation,
                        ):
                            coarse = reference(parameters, contract, 1e-10)
                            expected = reference(parameters, contract, 1e-12)
                            self.assertAlmostEqual(
                                coarse, expected, delta=2e-10 + 2e-12 * abs(expected)
                            )
                            actual = engine.price(contract, context(parameters))
                            fine = refined.price(contract, context(parameters))
                            self.assertAlmostEqual(
                                actual, expected, delta=1e-8 + 1e-8 * abs(expected)
                            )
                            self.assertAlmostEqual(
                                fine, expected, delta=2e-10 + 2e-11 * abs(expected)
                            )

    def test_shared_rust_python_anchors_and_parity(self):
        for parameters, expected in (
            (model(), 9.01127840754174),
            (
                model(mean_reversion_speed=0.2, vol_of_vol=1.0, correlation=-0.9),
                6.256279042682394,
            ),
        ):
            with self.subTest(price=expected):
                engine = AnalyticHestonEuropeanEngine()
                call = engine.price(option(), context(parameters))
                put = engine.price(option(option_type="put"), context(parameters))
                self.assertAlmostEqual(call, expected, delta=1e-8 + 1e-8 * expected)
                self.assertAlmostEqual(
                    call - put, 100 * (math.exp(-0.02) - math.exp(-0.05)), delta=1e-12
                )

    def test_tail_prices_have_relative_accuracy(self):
        # Independent QuantLib 1.43 reference is stable under 1e-10 -> 1e-12 refinement.
        contract = option(
            option_type="put", strike=70, expiry_time=START + timedelta(days=30)
        )
        expected = reference(model(), contract, 1e-12)
        self.assertAlmostEqual(expected, 3.105513031411244e-6, delta=2e-14)
        self.assertAlmostEqual(
            reference(model(), contract, 1e-10), expected, delta=2e-14
        )
        actual = AnalyticHestonEuropeanEngine(
            absolute_tolerance=0, relative_tolerance=1e-8, max_evaluations=40_000
        ).price(contract, context())
        self.assertGreater(actual, 0)
        self.assertLess(abs(actual / expected - 1), 1e-8)

    def test_zero_parameters_and_correlation_endpoints(self):
        # Exp-sinh quadrature verifies exact correlation endpoints; positive
        # limits verify zero parameters that QuantLib's model cannot construct.
        cases = (
            {"correlation": 1.0},
            {"correlation": -1.0},
            {"initial_variance": 0.0, "correlation": 0.0},
            {"mean_reversion_speed": 0.0},
            {"long_run_variance": 0.0},
            {"long_run_variance": 0.0, "mean_reversion_speed": 0.0, "correlation": 0.0},
        )
        for changes in cases:
            with self.subTest(changes=changes):
                actual = AnalyticHestonEuropeanEngine().price(
                    option(), context(model(**changes))
                )
                limits = []
                for epsilon in (1e-10, 1e-12):
                    interior = {
                        name: max(value, epsilon) if name != "correlation" else value
                        for name, value in changes.items()
                    }
                    limits.append(reference(model(**interior), option(), 1e-12))
                self.assertAlmostEqual(limits[0], limits[1], delta=1e-8)
                self.assertAlmostEqual(actual, limits[1], delta=1e-8)

    def test_deterministic_and_absorbing_variance_need_no_integral(self):
        engine = AnalyticHestonEuropeanEngine(max_evaluations=1)
        for v0, theta, kappa in (
            (0.04, 0.04, 1.5),
            (0.09, 0.01, 2),
            (0.04, 0.09, 0),
            (0, 0.04, 1.5),
        ):
            variance = (
                v0 if kappa == 0 else theta + (v0 - theta) * -math.expm1(-kappa) / kappa
            )
            bsm = BlackScholesMertonParameters(
                risk_free_rate=0.05, dividend_yield=0.02, volatility=math.sqrt(variance)
            )
            parameters = model(
                initial_variance=v0,
                long_run_variance=theta,
                mean_reversion_speed=kappa,
                vol_of_vol=0,
                correlation=1,
            )
            for kind in ("call", "put"):
                with self.subTest(v0=v0, theta=theta, kappa=kappa, kind=kind):
                    self.assertAlmostEqual(
                        engine.price(option(option_type=kind), context(parameters)),
                        AnalyticEuropeanEngine().price(
                            option(option_type=kind), context(bsm)
                        ),
                        delta=1e-12,
                    )
        zero = context(
            model(initial_variance=0, long_run_variance=0, mean_reversion_speed=0)
        )
        self.assertAlmostEqual(
            engine.price(option(), zero),
            100 * (math.exp(-0.02) - math.exp(-0.05)),
            delta=1e-12,
        )
        self.assertEqual(engine.price(option(option_type="put"), zero), 0)

    def test_small_vol_of_vol_converges_to_bsm_without_cancellation(self):
        expected = 9.227005508154036
        for xi in (1e-8, 1e-10, 1e-100):
            with self.subTest(xi=xi):
                price = AnalyticHestonEuropeanEngine().price(
                    option(), context(model(vol_of_vol=xi))
                )
                self.assertAlmostEqual(price, expected, delta=1e-8)

    def test_model_validation_replacement_and_owned_context(self):
        original = model()
        for name in PARAMETERS:
            for value in (None, True, "0.04"):
                with self.subTest(name=name, value=value), self.assertRaises(TypeError):
                    model(**{name: value})
        for name in (
            "initial_variance",
            "long_run_variance",
            "mean_reversion_speed",
            "vol_of_vol",
        ):
            for value in (-1e-16, math.inf, math.nan):
                with (
                    self.subTest(name=name, value=value),
                    self.assertRaises(InvalidInputError),
                ):
                    original.replace(**{name: value})
        for rho in (-1 - 1e-15, 1 + 1e-15, math.inf, math.nan):
            with (
                self.subTest(rho=rho),
                self.assertRaises(InvalidInputError),
            ):
                original.replace(correlation=rho)
        changed = original.replace(initial_variance=0.09, correlation=1)
        self.assertEqual(changed.initial_variance, 0.09)
        self.assertEqual(changed.correlation, 1)
        self.assertEqual(changed.long_run_variance, 0.04)
        self.assertEqual(original, model())
        defaults = {
            name: p.default
            for name, p in inspect.signature(original.replace).parameters.items()
        }
        self.assertEqual(original.replace(**defaults), original)
        for name in PARAMETERS:
            with self.subTest(name=name), self.assertRaises(TypeError):
                original.replace(**{name: None})
        snapshot = context(original)
        self.assertEqual(snapshot.model_parameters, original)
        self.assertIsNot(snapshot.model_parameters, original)
        self.assertEqual(
            snapshot.replace(model_parameters=changed).model_parameters, changed
        )
        with self.assertRaises(AttributeError):
            original.initial_variance = 0.09

    def test_numerical_controls_native_conversions_and_failure_state(self):
        engine = AnalyticHestonEuropeanEngine()
        self.assertEqual(
            (
                engine.absolute_tolerance,
                engine.relative_tolerance,
                engine.max_evaluations,
            ),
            (1e-8, 1e-8, 10000),
        )
        for name in ("absolute_tolerance", "relative_tolerance"):
            self.assertIs(
                inspect.signature(AnalyticHestonEuropeanEngine).parameters[name].kind,
                inspect.Parameter.KEYWORD_ONLY,
            )
            for value in (-1, math.inf, math.nan):
                with (
                    self.subTest(name=name, value=value),
                    self.assertRaises(InvalidInputError),
                ):
                    AnalyticHestonEuropeanEngine(**{name: value})
            with self.assertRaises(TypeError):
                AnalyticHestonEuropeanEngine(**{name: True})
        for controls in (
            {"absolute_tolerance": 0, "relative_tolerance": 0},
            {"max_evaluations": 0},
        ):
            with self.subTest(controls=controls), self.assertRaises(InvalidInputError):
                AnalyticHestonEuropeanEngine(**controls)
        for value, error in (
            (True, TypeError),
            (1.5, TypeError),
            (-1, OverflowError),
            (1 << 100, OverflowError),
        ):
            with self.subTest(value=value), self.assertRaises(error):
                AnalyticHestonEuropeanEngine(max_evaluations=value)
        snapshot = context()
        saved = snapshot.model_parameters
        limited = AnalyticHestonEuropeanEngine(max_evaluations=1)
        for _ in range(2):
            with self.assertRaises(NonConvergenceError):
                limited.price(option(), snapshot)
        self.assertEqual(limited.max_evaluations, 1)
        self.assertEqual(snapshot.model_parameters, saved)
        self.assertAlmostEqual(
            engine.price(option(), snapshot), 9.01127840754174, delta=1e-7
        )
        absolute_only = AnalyticHestonEuropeanEngine(relative_tolerance=0)
        self.assertAlmostEqual(
            absolute_only.price(option(), snapshot), 9.01127840754174, delta=1e-8
        )

    def test_time_expiry_and_unsupported_operations(self):
        engine = AnalyticHestonEuropeanEngine(max_evaluations=1)
        terminal = context(spot_price=110, valuation_time=END)
        self.assertEqual(engine.price(option(), terminal), 10)
        self.assertEqual(engine.price(option(option_type="put"), terminal), 0)
        for instant in (
            END + timedelta(days=1),
            datetime(2026, 1, 1, 0, 0, 0, 1),
        ):
            with self.subTest(instant=instant), self.assertRaises(InvalidInputError):
                engine.price(option(), context(valuation_time=instant))
        for snapshot in (context(), terminal):
            result = AnalyticHestonEuropeanEngine().price_with_greeks(
                option(), snapshot, "delta"
            )
            self.assertIsNone(result.delta)
            with self.assertRaises(UnsupportedOperationError):
                pricing.implied_volatility(engine, option(), snapshot, 9)
            with self.assertRaises(UnsupportedOperationError):
                AnalyticEuropeanEngine().price(option(), snapshot)
        with self.assertRaises(TypeError):
            context(valuation_time=datetime(2025, 1, 1).date())

    def test_curves_and_coverage_match_equivalent_flat_rates(self):
        risk = RateTermStructure(
            reference_time=START,
            times=[datetime(2025, 7, 1), END],
            factors=[0.99, math.exp(-0.05)],
        )
        dividend = RateTermStructure(
            reference_time=START, times=[END], factors=[math.exp(-0.02)]
        )
        parameters = model(risk_free_rate=risk, dividend_yield=dividend)
        with self.assertRaises(UnsupportedOperationError):
            _ = parameters.flat_risk_free_rate
        self.assertEqual(parameters.risk_free_curve.factors, risk.factors)
        actual = AnalyticHestonEuropeanEngine().price(option(), context(parameters))
        self.assertAlmostEqual(actual, 9.01127840754174, delta=1e-7)
        with self.assertRaises(InvalidInputError):
            AnalyticHestonEuropeanEngine().price(
                option(expiry_time=END + timedelta(days=1)), context(parameters)
            )
        with self.assertRaises(InvalidInputError):
            context(parameters, valuation_time=START - timedelta(days=1))

    def test_nonfinite_computation_has_its_own_error_category(self):
        parameters = model(initial_variance=1e308, long_run_variance=1e308)
        with self.assertRaises(ComputationFailureError):
            AnalyticHestonEuropeanEngine().price(
                option(expiry_time=datetime(2027, 1, 1)), context(parameters)
            )

    def test_shared_engine_supports_concurrent_independent_requests(self):
        engine = AnalyticHestonEuropeanEngine()
        snapshots = [context(spot_price=spot) for spot in (80, 100, 120)]
        snapshots.append(snapshots[1])
        expected = [engine.price(option(), snapshot) for snapshot in snapshots]
        with ThreadPoolExecutor(max_workers=3) as pool:
            actual = list(
                pool.map(lambda snapshot: engine.price(option(), snapshot), snapshots)
            )
        self.assertEqual(actual, expected)


if __name__ == "__main__":
    unittest.main()
