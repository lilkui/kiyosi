"""Expose Rust-derived defaults through Python signature introspection."""

from functools import wraps
from inspect import Signature, isclass, signature
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
    defaults_by_name = dict(native._signature_defaults)
    for name, value in vars(native).items():
        if isclass(value) and name.endswith("Engine"):
            instance = value()
            defaults_by_name[name] = {
                field: getattr(instance, field) for field in value._STATE_FIELDS
            }
    for name, defaults in defaults_by_name.items():
        value = getattr(native, name)
        if isclass(value):
            value.__signature__ = _signature(value, defaults)
        else:
            setattr(native, name, _wrap(value, defaults))
