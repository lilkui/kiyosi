"""Copy, replace and display immutable native values."""

import reprlib
from inspect import Parameter, Signature, isclass
from types import ModuleType
from typing import Any, get_args

from . import _native


def _copy(value: Any) -> Any:
    return value


def _deepcopy(value: Any, _memo: dict[int, Any]) -> Any:
    return value


def _fields(value: Any) -> dict[str, Any]:
    if isinstance(value, _native.GreekMethod):
        return {
            "name": value.name,
            "active_controls": value.active_controls,
        }
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
    if isinstance(value, _native.FiniteDifferenceSolution):
        return {
            name: getattr(value, name)
            for name in ("spot_range", "valuation_time", "supported")
        }
    if isinstance(value, _native.MonteCarloEstimate):
        return {
            "price": value.price,
            "standard_error": value.standard_error,
            "uncertainty_status": value.uncertainty_status,
            "seed": value.seed,
        }
    fields = {name: name for name in value._STATE_FIELDS}
    if isinstance(
        value,
        (
            _native.BlackScholesMertonParameters,
            _native.HestonParameters,
            _native.LocalVolatilityParameters,
        ),
    ):
        # Keep curve nodes, shifts and coverage visible.
        fields.update(risk_free_rate="risk_free_curve", dividend_yield="dividend_curve")
    return {name: getattr(value, attribute) for name, attribute in fields.items()}


def _replacement(factory: Any) -> Any:
    constructor = Signature(
        [
            Parameter(name, Parameter.KEYWORD_ONLY, default=Ellipsis)
            for name in factory._STATE_FIELDS
        ]
    )

    def replace(self: Any, **changes: Any) -> Any:
        """Return a validated copy with replacement fields.

        Parameters
        ----------
        **changes
            Constructor fields to replace; see the class documentation for field
            descriptions. Omitted fields and fields set to Ellipsis default to
            the instance's values. None clears nullable fields.

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
        return factory(**(_fields(self) | changes))

    replace.__doc__ = (replace.__doc__ or "").replace(
        "**changes", ", ".join(factory._STATE_FIELDS) + " : object, optional"
    )
    replace.__dict__["__signature__"] = constructor.replace(
        parameters=[
            Parameter("self", Parameter.POSITIONAL_OR_KEYWORD),
            *constructor.parameters.values(),
        ],
        return_annotation=factory,
    )
    return replace


_repr_formatter = reprlib.Repr()
_repr_formatter.maxother = 160


def _repr(value: Any) -> str:
    fields = _fields(value)
    name = type(value).__name__
    if fields.get("calendar") == _native.TradingCalendar.weekdays() and not isinstance(
        value,
        (
            _native.BlackScholesMertonParameters,
            _native.LocalVolatilityParameters,
            _native.HestonParameters,
        ),
    ):
        fields.pop("calendar")
    terms = ", ".join(
        f"{key}={_repr_formatter.repr(field)}" for key, field in fields.items()
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
            if value.__name__.endswith("Engine") and value._STATE_FIELDS:
                value.replace = _replacement(value)
            if value.__repr__ is object.__repr__:
                value.__repr__ = _repr
