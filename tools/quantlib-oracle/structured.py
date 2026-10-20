"""Price-only terminal portfolios; never import or call Kiyosi."""

import math
import os
import tempfile
from datetime import date
from pathlib import Path
from unittest.mock import patch

import oracle as g

FIXTURE = g.PROJECT.parents[1] / "tests/fixtures/structured_reference.tsv"
INSTRUMENTS = (
    "SnowballOption",
    "BinarySnowballOption",
    "TernarySnowballOption",
    "PhoenixOption",
)


def scenarios():
    for label, spot, rate, volatility, principal, coupon in (
        ("base", 100, 0.04, 0.2, 1, 0.08),
        ("signed", 80, -0.02, 0.6, 0.1, -0.2),
        ("high", 120, 0.07, 0.35, 1.25, 0.12),
    ):
        for instrument in INSTRUMENTS:
            histories = (
                ("none",)
                if instrument == "BinarySnowballOption"
                else ("none", "knocked_in")
            )
            for history in histories:
                yield (
                    f"{instrument.lower()}-{label}-{history}",
                    instrument,
                    {
                        "effective_date": "2025-01-01",
                        "valuation": "2025-01-01"
                        if history == "none"
                        else "2025-01-02",
                        "expiry_date": "2026-01-01",
                        "valuation_hour": 0,
                        "spot": spot,
                        "rate": rate,
                        "dividend": 0.01,
                        "volatility": volatility,
                        "initial_spot": 100,
                        "upper_strike": 100,
                        "lower_strike": 60,
                        "knock_in_level": 100,
                        "knock_out_level": 110,
                        "coupon_barrier_level": 95,
                        "principal_ratio": principal,
                        "coupon_rate": coupon,
                        "maturity_coupon_rate": 0.06,
                        "minimum_coupon_rate": 0.02,
                        "history": history,
                        "knock_in_mode": "at_expiry"
                        if history == "none"
                        else "every_trading_day",
                        "exposure": 1,
                        "convergence": str(
                            label == "base" and history == "none"
                        ).lower(),
                    },
                )
    for volatility, rate in ((0.2, 0.04), (0.4, -0.02)):
        for barrier in (90, 100, 110):
            for acceleration in (0, 1, 2):
                yield (
                    f"accumulator-{volatility}-{barrier}-{acceleration}",
                    "Accumulator",
                    {
                        "effective_date": "2025-01-01",
                        "valuation": "2025-01-03",
                        "expiry_date": "2025-01-06",
                        "valuation_hour": 12,
                        "spot": 100,
                        "rate": rate,
                        "dividend": 0.01,
                        "volatility": volatility,
                        "strike": 100,
                        "knock_out_level": barrier,
                        "daily_quantity": 1,
                        "acceleration_factor": acceleration,
                        "accumulated_quantity": 3,
                        "exposure": 100 * (3 + max(1, acceleration)),
                        "convergence": str(
                            volatility == 0.2 and acceleration == 2
                        ).lower(),
                    },
                )


def horizon(inputs, field="valuation"):
    days = (
        date.fromisoformat(inputs["expiry_date"]) - date.fromisoformat(inputs[field])
    ).days
    return (days - (inputs["valuation_hour"] / 24 if field == "valuation" else 0)) / 365


def portfolio(instrument, inputs):
    """Return constant cash, constant asset units, and (kind, direction, strike, weight) legs."""
    legs = []

    def leg(kind, direction, strike, weight):
        legs.append((kind, direction, strike, weight))

    if instrument == "Accumulator":
        strike, barrier = inputs["strike"], inputs["knock_out_level"]
        daily, acceleration = inputs["daily_quantity"], inputs["acceleration_factor"]
        quantity = inputs["accumulated_quantity"]
        for level, weight in (
            ((barrier, daily), (strike, daily * (acceleration - 1)))
            if barrier > strike
            else ((barrier, daily * acceleration),)
        ):
            leg("asset", "put", level, weight)
            leg("cash", "put", level, -strike * weight)
        return -quantity * strike, quantity, legs

    time = horizon(inputs, "effective_date")
    principal = inputs["principal_ratio"]
    coupon, maturity, minimum = (
        inputs[name] * time
        for name in ("coupon_rate", "maturity_coupon_rate", "minimum_coupon_rate")
    )
    barrier, knock_in = inputs["knock_out_level"], inputs["knock_in_level"]
    known_knock_in = inputs["history"] == "knocked_in"
    if instrument == "BinarySnowballOption":
        leg("cash", "call", barrier, coupon - maturity)
        return principal + maturity, 0, legs
    if instrument == "TernarySnowballOption":
        if not known_knock_in:
            leg("cash", "call", knock_in, maturity - minimum)
        leg("cash", "call", barrier, coupon - (minimum if known_knock_in else maturity))
        return principal + minimum, 0, legs

    leg("vanilla", "put", inputs["upper_strike"], -1 / inputs["initial_spot"])
    leg("vanilla", "put", inputs["lower_strike"], 1 / inputs["initial_spot"])
    if instrument == "PhoenixOption":
        leg("cash", "call", inputs["coupon_barrier_level"], coupon)
    else:
        if not known_knock_in:
            leg("cash", "call", knock_in, maturity)
        leg("cash", "call", barrier, coupon - (0 if known_knock_in else maturity))
    return principal, 0, legs


def payoff(kind, direction, strike):
    option = g.ql.Option.Call if direction == "call" else g.ql.Option.Put
    if kind == "cash":
        return g.ql.CashOrNothingPayoff(option, strike, 1)
    if kind == "asset":
        return g.ql.AssetOrNothingPayoff(option, strike)
    return g.ql.PlainVanillaPayoff(option, strike)


def price(instrument, inputs):
    cash, asset, legs = portfolio(instrument, inputs)
    time = horizon(inputs)
    discount = math.exp(-inputs["rate"] * time)
    forward = inputs["spot"] * math.exp((inputs["rate"] - inputs["dividend"]) * time)
    deviation = inputs["volatility"] * math.sqrt(time)
    return discount * (cash + asset * forward) + math.fsum(
        weight
        * g.ql.BlackCalculator(
            payoff(kind, direction, strike), forward, deviation, discount
        ).value()
        for kind, direction, strike, weight in legs
    )


def rows():
    for identifier, instrument, inputs in scenarios():
        reference = price(instrument, inputs)
        g.require(math.isfinite(reference), "non-finite structured reference")
        accumulator = instrument == "Accumulator"
        for method in ("Quadrature", "FiniteDifference"):
            engine = method + (
                "AccumulatorEngine" if accumulator else "AutocallableEngine"
            )
            tolerance = (
                1e-9
                if method == "Quadrature"
                else (1e-6 * inputs["exposure"] if accumulator else 1e-4)
            )
            settings = (
                {"asset_step_count": 512, "integration_standard_deviations": 8}
                if method == "Quadrature"
                else {
                    "asset_step_count": 17600 if accumulator else 800,
                    "time_step_count": 800 if accumulator else 1600,
                    "asset_upper_boundary": 440 if accumulator else 400,
                    "scheme": "crank_nicolson",
                }
            )
            yield {
                "case_id": f"portfolio-{identifier}-{method.lower()}",
                "instrument": instrument,
                "engine": engine,
                "variant": "single-future-observation",
                "inputs": dict(
                    inputs,
                    **settings,
                    owner="KiyosiValidation",
                    source_revision=f"terminal-portfolio-v1/QuantLib-{g.ql.__version__}",
                    source_symbol="QuantLib.BlackCalculator",
                    convention=g.CONVENTION,
                    calendar="weekends_only",
                    reference_kind="analytic",
                    reference_classification="quantlib-analytic-portfolio",
                    tolerance=tolerance,
                    unit_price="price",
                    uncertainty_price=1e-12 * max(1, inputs["exposure"]),
                    reference_uncertainty="floating-point allowance, not a rigorous bound",
                ),
                "outputs": {"price": reference},
                "tolerances": {"price": tolerance},
            }


def content():
    g.require(g.ql.__version__ == "1.43", "run with the frozen uv environment")
    generated = sorted(rows(), key=lambda row: row["case_id"])
    g.require(
        len({row["case_id"] for row in generated}) == len(generated),
        "duplicate portfolio case",
    )
    return "\n".join(
        [
            "# Generated by tools/quantlib-oracle/structured.py: QuantLib basic-payoff portfolios, not native structured engines.",
            "\t".join(g.HEADER),
            *(g.serialize_row(row) for row in generated),
            "",
        ]
    )


def regenerate(fixture=FIXTURE):
    encoded = content().encode("utf-8")
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(dir=fixture.parent, delete=False) as output:
            temporary = Path(output.name)
            output.write(encoded)
        os.replace(temporary, fixture)
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


def check_generation():
    # Check the portfolio identity off thresholds; exact equality is tested against core events.
    for _, instrument, inputs in scenarios():
        cash, asset, legs = portfolio(instrument, inputs)
        for spot in (50, 79, 89, 94, 99, 101, 109, 111, 150):
            value = (
                cash
                + asset * spot
                + math.fsum(
                    weight * payoff(kind, direction, strike)(spot)
                    for kind, direction, strike, weight in legs
                )
            )
            if instrument == "Accumulator":
                quantity = inputs["accumulated_quantity"]
                if spot < inputs["knock_out_level"]:
                    quantity += inputs["daily_quantity"] * (
                        inputs["acceleration_factor"] if spot < inputs["strike"] else 1
                    )
                expected = quantity * (spot - inputs["strike"])
            else:
                time = horizon(inputs, "effective_date")
                expected = inputs["principal_ratio"]
                knocked_in = (
                    inputs["history"] == "knocked_in" or spot < inputs["knock_in_level"]
                )
                if (
                    instrument == "PhoenixOption"
                    and spot >= inputs["coupon_barrier_level"]
                ):
                    expected += inputs["coupon_rate"] * time
                if spot >= inputs["knock_out_level"]:
                    if instrument != "PhoenixOption":
                        expected += inputs["coupon_rate"] * time
                elif instrument in ("SnowballOption", "PhoenixOption") and knocked_in:
                    expected += (
                        min(inputs["upper_strike"], max(inputs["lower_strike"], spot))
                        - inputs["upper_strike"]
                    ) / inputs["initial_spot"]
                elif instrument == "TernarySnowballOption" and knocked_in:
                    expected += inputs["minimum_coupon_rate"] * time
                elif instrument != "PhoenixOption":
                    expected += inputs["maturity_coupon_rate"] * time
            assert math.isclose(value, expected, rel_tol=1e-13, abs_tol=1e-12), (
                instrument,
                inputs,
                spot,
                value,
                expected,
            )
    assert content().encode("utf-8") == FIXTURE.read_bytes(), (
        "structured reference fixture is stale"
    )
    assert any(row["outputs"]["price"] < 0 for row in rows()), (
        "signed portfolio coverage missing"
    )
    with tempfile.TemporaryDirectory() as directory:
        fixture = Path(directory) / "reference.tsv"
        fixture.write_bytes(b"preserve")
        with patch(__name__ + ".price", return_value=math.nan):
            try:
                regenerate(fixture)
            except ValueError:
                pass
            else:
                raise AssertionError("non-finite reference accepted")
        assert fixture.read_bytes() == b"preserve", (
            "failed calculation replaced fixture"
        )


if __name__ == "__main__":
    regenerate()
