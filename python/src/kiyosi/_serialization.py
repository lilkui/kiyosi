"""Copy and reconstruct immutable native values through validated constructors."""

import copyreg
from datetime import datetime
from inspect import isclass, signature
from types import ModuleType
from typing import Any, get_args

from . import _native


def _copy(value: Any) -> Any:
    return value


def _deepcopy(value: Any, _memo: dict[int, Any]) -> Any:
    return value


def _restore(factory: Any, fields: dict[str, Any]) -> Any:
    return factory(**fields)


def _calendar(
    kind: str, base: Any, closed_dates: Any, open_dates: Any, trading_days_per_year: int
) -> Any:
    if base is None:
        base = {
            "weekdays": _native.weekdays_calendar,
            "all_days": _native.all_days_calendar,
            "sse": _native.sse_calendar,
        }[kind]()
    return _native.custom_calendar(
        base=base,
        closed_dates=closed_dates,
        open_dates=open_dates,
        trading_days_per_year=trading_days_per_year,
    )


def _curve(reference_time: Any, times: Any, factors: Any, shift: float) -> Any:
    return _native.RateTermStructure(
        reference_time=reference_time, times=times, factors=factors
    ).shifted(shift)


def _reduce(value: Any) -> tuple[Any, tuple[Any, ...]]:
    factory = type(value)
    if isinstance(value, _native.TradingCalendar):
        kind, base, closed, opened, basis = _native._calendar_parts(value)
        return _calendar, (kind, base, closed, opened, basis)
    if isinstance(value, _native.ObservationSchedule):
        return _restore, (_native._observation_schedule, {"times": value.times})
    if isinstance(value, _native.RateTermStructure):
        if value.is_explicitly_flat:
            fields = {
                "rate": value.forward_rate(value.reference_time or datetime(1, 1, 1)),
                "reference_time": value.reference_time,
            }
            return _restore, (_native.RateTermStructure.flat, fields)
        factors, shift = _native._curve_parts(value)
        return _curve, (value.reference_time, value.nodes, factors, shift)
    if isinstance(value, _native.PricingResult):
        values = {name: getattr(value, name) for name in get_args(_native.Greek)}
        return _restore, (
            _native._pricing_result,
            {
                "price": value.price,
                "requested": value.requested,
                "supported": value.supported,
                "greeks": {
                    name: result
                    for name, result in values.items()
                    if result is not None
                },
            },
        )
    if isinstance(value, _native.MonteCarloEstimate):
        return _restore, (
            _native._monte_carlo_estimate,
            {"price": value.price, "standard_error": value.standard_error},
        )
    if isinstance(value, _native.BinaryBarrierOption):
        factory = (
            _native.cash_binary_barrier_option
            if value.payoff_type == "cash"
            else _native.asset_binary_barrier_option
        )
    elif isinstance(value, _native.TouchOption):
        factory = (
            _native.cash_touch_option
            if value.payoff_type == "cash"
            else _native.asset_touch_option
        )
    elif isinstance(value, _native.DoubleTouchOption):
        factory = (
            _native.cash_double_touch
            if value.is_one_touch
            else _native.cash_double_no_touch
        )
    fields = {name: getattr(value, name) for name in signature(factory).parameters}
    if isinstance(value, _native.PhoenixOption):
        for name in ("coupon_rate", "coupon_per_period"):
            if fields[name] is None:
                del fields[name]
    return _restore, (factory, fields)


def install(native: ModuleType) -> None:
    for value in tuple(vars(native).values()):
        if (
            isclass(value)
            and value.__module__ == native.__name__
            and not issubclass(value, Exception)
        ):
            value.__copy__ = _copy
            value.__deepcopy__ = _deepcopy
            copyreg.pickle(value, _reduce)
