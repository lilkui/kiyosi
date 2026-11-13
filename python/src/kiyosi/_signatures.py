"""Expose Rust-derived defaults through Python signature introspection."""

from functools import wraps
from inspect import Parameter, Signature, isbuiltin, isclass, signature
from types import ModuleType
from typing import Any

from ._serialization import _RATE_FIELDS


def _signature(callable: Any, defaults: dict[str, Any]) -> Signature:
    original = signature(callable)
    defaults = defaults.copy()
    if {
        "times",
        "autocall_times",
        "observation_times",
        "knock_out_observation_times",
    } & original.parameters.keys():
        defaults.pop("calendar", None)
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


def _replacement(factory: Any) -> Any:
    constructor = signature(factory)

    def replace(self: Any, **changes: Any) -> Any:
        """Return a validated copy with replacement fields.

        The original value is unchanged on success and failure.

        Parameters
        ----------
        **changes : object, optional
            Constructor fields to replace; see the class documentation for field
            descriptions. Omission or Ellipsis retains a field; None clears
            nullable fields.

        Returns
        -------
        object
            Validated instance of the same class.

        Raises
        ------
        InvalidInputError
            If fields fail constructor validation.
        TypeError
            If a field name or representation is invalid.
        """
        constructor.bind_partial(**changes)
        changes = {
            name: value for name, value in changes.items() if value is not Ellipsis
        }
        controls = {
            name: changes[name]
            if name in changes
            else getattr(self, _RATE_FIELDS.get(name, name))
            for name in constructor.parameters
        }
        return factory(**controls)

    replace.__dict__["__signature__"] = constructor.replace(
        parameters=[
            Parameter("self", Parameter.POSITIONAL_OR_KEYWORD),
            *(
                parameter.replace(default=Ellipsis)
                for parameter in constructor.parameters.values()
            ),
        ],
        return_annotation=factory,
    )
    return replace


def install(native: ModuleType) -> None:
    defaults = dict(native._signature_defaults)
    defaults["calendar"] = native.weekdays_calendar()
    defaults.update(closed_dates=(), open_dates=())
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
            resolved = _signature(value, constructor_defaults)
            if resolved != original:
                value.__signature__ = resolved
            if name.endswith("Engine") or name in (
                "EuropeanOption",
                "AmericanOption",
                "CashOrNothingOption",
                "AssetOrNothingOption",
                "GeometricAveragePriceOption",
                "ArithmeticAveragePriceOption",
                "BlackScholesMertonParameters",
                "HestonParameters",
                "LocalVolatilityParameters",
                "PricingContext",
            ):
                value.replace = _replacement(value)
        elif isbuiltin(value) and name != "numerical_greeks":
            setattr(native, name, _wrap(value, solvers.get(name, defaults)))
