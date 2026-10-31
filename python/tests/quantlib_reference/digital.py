"""European digital references using QuantLib's analytic European engine."""

from datetime import date, timedelta

from . import oracle as g

INSTRUMENTS = {
    "cash": "CashOrNothingOption",
    "asset": "AssetOrNothingOption",
}
# Whole-day time stencils see greater curvature for digital asset payouts.
STABILITY = dict(
    g.STABILITY,
    theta=0.0003,
    charm=0.0002,
    color=0.00001,
    delta=0.00001,
    gamma=0.000001,
    speed=0.00001,
    vega=0.00001,
    vanna=0.000001,
    zomma=0.000001,
    rho=0.00001,
)


def scenarios(config):
    for kind in INSTRUMENTS:
        for direction in ("call", "put"):
            for spot, days in [
                (spot, days) for spot in (80, 100, 120) for days in (30, 365, 730)
            ] + [(100, 1)]:
                inputs = dict(
                    config["inputs"],
                    option=direction,
                    spot=spot,
                    expiry_date=(
                        date.fromisoformat(config["inputs"]["valuation"])
                        + timedelta(days=days)
                    ).isoformat(),
                )
                yield (
                    f"ql-digital-{kind}-{direction}-{spot}-{days}d",
                    dict(
                        inputs,
                        payoff=kind,
                        **({"payout": config["payout"]} if kind == "cash" else {}),
                    ),
                )
