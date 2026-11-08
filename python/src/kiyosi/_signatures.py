"""Expose Rust-derived defaults through Python signature introspection."""

from functools import wraps
from inspect import Parameter, Signature, isbuiltin, isclass, signature
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


def _engine_replacement(factory: Any) -> Any:
    constructor = signature(factory)

    def replace(self: Any, **changes: Any) -> Any:
        """Return a validated engine; omitted controls or Ellipsis retain their values.

        Explicit None clears nullable controls. Constructor validation applies to
        supplied controls, and the original engine is preserved on success and failure.
        """
        constructor.bind_partial(**changes)
        controls = {name: getattr(self, name) for name in constructor.parameters}
        controls.update(
            (name, value) for name, value in changes.items() if value is not Ellipsis
        )
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
    defaults.update(observation_times=(), closed_dates=(), open_dates=())
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
            if name.endswith("Engine"):
                value.replace = _engine_replacement(value)
        elif isbuiltin(value):
            setattr(native, name, _wrap(value, solvers.get(name, defaults)))
