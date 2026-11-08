"""Copy and reconstruct immutable native values through validated constructors."""

import copyreg
import reprlib
from datetime import datetime
from inspect import isclass, signature
from types import GetSetDescriptorType, ModuleType
from typing import Any, get_args

from . import _native

_COLLECTION_PROPERTIES = {
    "base_factors",
    "coupon_barrier_levels",
    "factors",
    "knock_out_coupon_rates",
    "knock_out_levels",
    "knock_out_observation_times",
    "nodes",
    "observation_times",
    "observations",
    "requested",
    "spots",
    "supported",
    "times",
    "values",
}


def _immutable_collection(value: Any) -> Any:
    return (
        tuple(_immutable_collection(item) for item in value)
        if isinstance(value, list)
        else value
    )


def _collection_property(descriptor: Any) -> property:
    def get(value: Any) -> Any:
        return _immutable_collection(descriptor.__get__(value, type(value)))

    return property(get, doc=descriptor.__doc__)


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


def _reduce(
    value: Any, previews: dict[str, str] | None = None
) -> tuple[Any, tuple[Any, ...]]:
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
                "calendar": _native.all_days_calendar(),
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
    if isinstance(value, _native.PricingResult):
        values = {name: getattr(value, name) for name in get_args(_native.Greek)}
        return _restore, (
            _native._pricing_result,
            {
                "price": value.price,
                "requested": value.requested,
                "supported": value.supported,
                "unavailable_stencils": [
                    name
                    for name in value.requested
                    if value.status(name) == "stencil_unavailable"
                ],
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
            {
                "price": value.price,
                "standard_error": value.standard_error,
                "uncertainty_status": value.uncertainty_status,
                "seed": value.seed,
            },
        )
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
    rate_fields = {
        "risk_free_rate": "risk_free_curve",
        "dividend_yield": "dividend_curve",
    }
    fields = {
        name: _Preview(previews[name])
        if previews and name in previews
        else getattr(value, rate_fields.get(name, name))
        for name in signature(factory).parameters
    }
    return _restore, (factory, fields)


_repr_formatter = reprlib.Repr()
_repr_formatter.maxother = 160


def _repr(value: Any) -> str:
    previews = _native._repr_previews(value)
    if "__repr__" in previews:
        return previews["__repr__"]
    factory, args = _reduce(value, previews)
    name = type(value).__name__
    if factory is _calendar:
        kind, base, closed, opened, basis = args
        fields = {"kind": kind, "trading_days_per_year": basis}
        if base is not None:
            fields.update(base=base, closed_dates=closed, open_dates=opened)
        suffix = ""
    elif factory is _curve:
        reference, times, factors, shift = args
        fields = {"reference_time": reference, "times": times, "factors": factors}
        suffix = f".shifted({shift!r})"
    else:
        factory, fields = args
        fields = fields.copy()
        if isinstance(value, _native.PhoenixOption):
            for coupon_name in ("coupon_rate", "coupon_per_period"):
                if fields[coupon_name] is None:
                    del fields[coupon_name]
        if fields.get("calendar") == _native.weekdays_calendar():
            fields.pop("calendar")
        suffix = ""
        if isinstance(value, _native.RateTermStructure):
            name += ".flat"
    terms = ", ".join(
        f"{key}={field.text if isinstance(field, _Preview) else _repr_formatter.repr(field)}"
        for key, field in fields.items()
    )
    return f"{name}({terms}){suffix}"


def install(native: ModuleType) -> None:
    for value in tuple(vars(native).values()):
        if (
            isclass(value)
            and value.__module__ == native.__name__
            and not issubclass(value, Exception)
        ):
            for name, descriptor in tuple(vars(value).items()):
                if name in _COLLECTION_PROPERTIES and isinstance(
                    descriptor, GetSetDescriptorType
                ):
                    setattr(value, name, _collection_property(descriptor))
            value.__copy__ = _copy
            value.__deepcopy__ = _deepcopy
            copyreg.pickle(value, _reduce)
            if value.__repr__ is object.__repr__:
                value.__repr__ = _repr
