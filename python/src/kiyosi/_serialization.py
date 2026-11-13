"""Copy and reconstruct immutable native values through validated constructors."""

import copyreg
import reprlib
from datetime import datetime
from functools import cache
from inspect import Parameter, Signature, isclass, signature
from types import ModuleType
from typing import Any, get_args

from . import _native


@cache
def _constructor(factory: Any) -> Signature:
    return signature(factory)


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


class _Preview:
    def __init__(self, text: str) -> None:
        self.text = text

    def __repr__(self) -> str:
        return self.text


def _reduce(value: Any) -> tuple[Any, tuple[Any, ...]]:
    factory = type(value)
    if isinstance(value, _native.TradingCalendar):
        kind, base, closed, opened, basis = _native._calendar_parts(value)
        return _calendar, (kind, base, closed, opened, basis)
    if isinstance(value, _native.ObservationSchedule):
        times = value.times
        return _restore, (
            _native.ObservationSchedule,
            {
                "times": times,
                "start_time": times[0] if times else datetime.min,
                "end_time": times[-1] if times else datetime.min,
                "calendar": value.calendar,
            },
        )
    if isinstance(value, _native.RateTermStructure):
        if not value.nodes:
            fields = {
                "rate": value.forward_rate(value.reference_time or datetime(1, 1, 1)),
                "reference_time": value.reference_time,
            }
            if value.max_time is not None:
                fields["max_time"] = value.max_time
                return _restore, (_native.RateTermStructure.bounded_flat, fields)
            return _restore, (_native.RateTermStructure.flat, fields)
        return _curve, (
            value.reference_time,
            value.nodes,
            value.base_factors,
            value.parallel_shift,
        )
    if isinstance(value, _native.KnockInObservation):
        factory = getattr(_native.KnockInObservation, value.kind)
        fields = (
            {"observation_clock_time": value.observation_clock_time}
            if value.kind == "every_trading_day"
            else {}
        )
        return _restore, (factory, fields)
    if isinstance(value, _native.AutocallableObservation):
        field = "coupon_barrier_level" if value.kind == "phoenix" else "coupon_rate"
        factory = getattr(_native.AutocallableObservation, value.kind)
        return _restore, (
            factory,
            {
                "time": value.time,
                "knock_out_level": value.knock_out_level,
                field: getattr(value, field),
            },
        )
    if isinstance(value, _native.PricingResult):
        factory = _native._pricing_result
    elif isinstance(value, _native.MonteCarloEstimate):
        factory = _native._monte_carlo_estimate
    return _restore, (factory, _fields(value))


def _fields(value: Any, previews: dict[str, str] | None = None) -> dict[str, Any]:
    if isinstance(value, _native.PricingResult):
        values = {name: getattr(value, name) for name in get_args(_native.Greek)}
        return {
            "price": value.price,
            "seed": value.seed,
            "requested": value.requested,
            "supported": value.supported,
            "unavailable_stencils": [
                name
                for name in value.requested
                if value.status(name) == "stencil_unavailable"
            ],
            "greeks": {
                name: result for name, result in values.items() if result is not None
            },
        }
    if isinstance(value, _native.MonteCarloEstimate):
        return {
            "price": value.price,
            "standard_error": value.standard_error,
            "uncertainty_status": value.uncertainty_status,
            "seed": value.seed,
        }
    rate_fields = {
        "risk_free_rate": "risk_free_curve",
        "dividend_yield": "dividend_curve",
    }
    return {
        name: _Preview(previews[name])
        if previews and name in previews
        else getattr(value, rate_fields.get(name, name))
        for name in _constructor(type(value)).parameters
    }


_repr_formatter = reprlib.Repr()
_repr_formatter.maxother = 160


def _repr(value: Any) -> str:
    previews = _native._repr_previews(value)
    if "__repr__" in previews:
        return previews["__repr__"]
    fields = _fields(value, previews)
    name = type(value).__name__
    if fields.get("calendar") == _native.weekdays_calendar():
        if _constructor(type(value)).parameters["calendar"].default is Parameter.empty:
            fields["calendar"] = _Preview("weekdays_calendar()")
        else:
            fields.pop("calendar")
    terms = ", ".join(
        f"{key}={field.text if isinstance(field, _Preview) else _repr_formatter.repr(field)}"
        for key, field in fields.items()
    )
    return f"{name}({terms})"


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
            if value.__repr__ is object.__repr__:
                value.__repr__ = _repr
