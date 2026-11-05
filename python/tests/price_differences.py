"""Independent scalar-price references for tests with feasible stencils."""

from datetime import datetime, timedelta
from functools import cache
from types import SimpleNamespace

from kiyosi.instruments import EuropeanOption
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
        "volga",
        "vanna",
        "zomma",
        "rho",
        "dividend_rho",
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
        parameters.flat_risk_free_rate,
    )
    h = (spot + spot_shift) - spot
    v = (volatility + volatility_shift) - volatility
    r = (rate + rate_shift) - rate
    q = (parameters.flat_dividend_yield + rate_shift) - parameters.flat_dividend_yield
    time = context.valuation_time
    start = datetime.combine(option.effective_time, datetime.min.time())
    end = datetime.combine(option.expiry_time, datetime.min.time())
    step = timedelta(days=time_shift_days)
    if start < time < end and not isinstance(option, EuropeanOption):
        step = min(step, time - start, end - time)
    before, after = (
        (-min(step, end - time), min(step, end - time))
        if isinstance(option, EuropeanOption)
        else (-min(step, time - start), min(step, end - time))
    )
    years = (after - before).total_seconds() / (365 * 86400)

    @cache
    def price(ds=0, dv=0, dr=0, dt=timedelta(), dq=0):
        return engine.price(
            option,
            PricingContext(
                model_parameters=BlackScholesMertonParameters(
                    risk_free_rate=rate + dr,
                    dividend_yield=parameters.flat_dividend_yield + dq,
                    volatility=volatility + dv,
                ),
                spot_price=spot + ds,
                valuation_time=time + dt,
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
        "vega": lambda: (price(dv=v) - price(dv=-v)) / (2 * v),
        "volga": lambda: (price(dv=v) - 2 * price() + price(dv=-v)) / v**2,
        "vanna": lambda: (
            ((price(h, v) - price(-h, v)) - (price(h, -v) - price(-h, -v))) / h / v / 4
        ),
        "zomma": lambda: (curvature(v) / h / h - curvature(-v) / h / h) / (2 * v),
        "rho": lambda: (price(dr=r) - price(dr=-r)) / (2 * r),
        "dividend_rho": lambda: (price(dq=q) - price(dq=-q)) / (2 * q),
        "theta": lambda: (price(dt=after) - price(dt=before)) / years,
        "charm": lambda: (spread(dt=after) - spread(dt=before)) / h / years / 2,
        "color": lambda: (curvature(dt=after) - curvature(dt=before)) / h / h / years,
    }
    return SimpleNamespace(
        price=engine.price(option, context),
        **{name: measures[name]() for name in greeks},
    )
