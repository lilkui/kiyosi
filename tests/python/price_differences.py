"""Independent scalar-price references for tests with feasible stencils."""

from datetime import UTC, datetime, timedelta
from functools import cache
from types import SimpleNamespace

from kiyosi.market import BlackScholesMertonParameters, PricingContext


def price_differences(
    engine,
    option,
    context,
    greeks=(
        "delta",
        "gamma",
        "speed",
        "theta",
        "charm",
        "color",
        "vega",
        "vanna",
        "zomma",
        "rho",
    ),
    *,
    spot_shift=0.01,
    volatility_shift=0.0001,
    rate_shift=0.0001,
    time_shift_days=1,
):
    parameters = context.model_parameters
    spot, volatility, rate = (
        context.spot_price,
        parameters.volatility,
        parameters.risk_free_rate,
    )
    h = (spot + spot_shift) - spot
    v = (volatility + volatility_shift) - volatility
    r = (rate + rate_shift) - rate
    time = context.valuation_time
    start = datetime.combine(option.effective_date, datetime.min.time(), UTC)
    end = datetime.combine(option.expiry_date, datetime.min.time(), UTC)
    step = timedelta(days=time_shift_days)
    if start < time < end:
        step = min(step, time - start, end - time)
    before, after = -min(step, time - start), min(step, end - time)
    days = (after - before).total_seconds() / 86400

    @cache
    def price(ds=0, dv=0, dr=0, dt=timedelta()):
        return engine.price(
            option,
            PricingContext(
                model_parameters=BlackScholesMertonParameters(
                    risk_free_rate=rate + dr,
                    dividend_yield=parameters.dividend_yield,
                    volatility=volatility + dv,
                ),
                spot_price=spot + ds,
                valuation_time=time + dt,
                calendar=context.calendar,
            ),
        )

    def spread(dv=0, dt=timedelta()):
        return price(h, dv, dt=dt) - price(-h, dv, dt=dt)

    def curvature(dv=0, dt=timedelta()):
        return price(h, dv, dt=dt) - 2 * price(dv=dv, dt=dt) + price(-h, dv, dt=dt)

    measures = {
        "delta": lambda: spread() / (2 * h),
        "gamma": lambda: curvature() / h / h,
        "speed": lambda: (
            (price(2 * h) - 2 * price(h) + 2 * price(-h) - price(-2 * h))
            / h
            / h
            / (2 * h)
        ),
        "vega": lambda: (price(dv=v) - price(dv=-v)) / (200 * v),
        "vanna": lambda: (
            ((price(h, v) - price(-h, v)) - (price(h, -v) - price(-h, -v)))
            / h
            / (100 * v)
            / 4
        ),
        "zomma": lambda: (curvature(v) / h / h - curvature(-v) / h / h) / (200 * v),
        "rho": lambda: (price(dr=r) - price(dr=-r)) / (200 * r),
        "theta": lambda: (price(dt=after) - price(dt=before)) / days,
        "charm": lambda: (spread(dt=after) - spread(dt=before)) / h / days / 2,
        "color": lambda: (curvature(dt=after) - curvature(dt=before)) / h / h / days,
    }
    return SimpleNamespace(
        price=engine.price(option, context),
        **{name: measures[name]() for name in greeks},
    )
