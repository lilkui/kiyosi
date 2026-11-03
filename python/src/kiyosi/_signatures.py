"""Expose Rust-derived defaults through Python signature introspection."""

from functools import wraps
from inspect import Signature, isbuiltin, isclass, signature
from types import ModuleType
from typing import Any


def _signature(callable: Any, defaults: dict[str, Any]) -> Signature:
    original = signature(callable)
    return original.replace(
        parameters=[
            parameter.replace(default=defaults[name])
            if parameter.default is Ellipsis and name in defaults
            else parameter
            for name, parameter in original.parameters.items()
        ]
    )


def _wrap(callable: Any, defaults: dict[str, Any]) -> Any:
    resolved = _signature(callable, defaults)
    if resolved == signature(callable):
        return callable

    # Native functions/descriptors cannot carry __signature__; preserve their validation.
    @wraps(callable)
    def call(*args: Any, **kwargs: Any) -> Any:
        return callable(*args, **kwargs)

    call.__dict__["__signature__"] = resolved
    return call


def install(native: ModuleType) -> None:
    defaults = dict(native._signature_defaults)
    defaults["calendar"] = native.weekdays_calendar()
    defaults.update(observation_dates=(), closed_dates=(), open_dates=())
    solvers = native._solver_signature_defaults
    for name, value in tuple(vars(native).items()):
        if name.startswith("_"):
            continue
        if isclass(value) and value.__module__ == native.__name__:
            try:
                original = signature(value)
            except (TypeError, ValueError):
                continue
            constructor_defaults = defaults.copy()
            if name.endswith("Engine") and any(
                p.default is Ellipsis for p in original.parameters.values()
            ):
                engine = value()
                constructor_defaults.update(
                    (key, getattr(engine, key))
                    for key, p in original.parameters.items()
                    if p.default is Ellipsis
                )
            if name == "PhoenixOption":
                constructor_defaults.pop("coupon_rate", None)
            resolved = _signature(value, constructor_defaults)
            if resolved != original:
                value.__signature__ = resolved
            if hasattr(value, "price_with_greeks"):
                method_defaults = defaults
                if name in ("MonteCarloBarrierEngine", "MonteCarloDoubleBarrierEngine"):
                    method_defaults = defaults | {
                        "spot_shift": defaults["fixed_spot_shift"]
                    }
                value.price_with_greeks = _wrap(
                    value.price_with_greeks, method_defaults
                )
        elif isbuiltin(value):
            setattr(native, name, _wrap(value, solvers.get(name, defaults)))
