"""Python API for the kiyosi derivatives pricing core."""

from typing import Literal

from ._native import ErrorCategory, KiyosiError, PricingResult, __version__

Greek = Literal[
    "delta", "gamma", "speed", "theta", "charm", "color", "vega",
    "vanna", "zomma", "rho",
]

__all__ = [
    "ErrorCategory",
    "KiyosiError",
    "PricingResult",
    "Greek",
    "__version__",
]
