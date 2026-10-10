"""Python API for the kiyosi derivatives pricing core.

Failures and cancellation
-------------------------
Representation and signature errors use Python's built-in exceptions. Core domain
rejections and reported backend failures raise KiyosiError; inspect its category.
CUDA unavailability and execution failures use BACKEND_UNAVAILABLE and
BACKEND_FAILURE, and invalid numerical results use INVALID_RESULT.

Allocation failures, including CUDA device-memory exhaustion, raise MemoryError.
Other native runtime/internal exceptions use the binding's built-in exception
mappings; for example, entropy-source runtime failures raise RuntimeError.
Valid settings do not guarantee sufficient memory or other resources.

Pricing, Greeks and implied-value solves are synchronous and release the GIL during
core computation. They provide no cancellation interface or signal polling within
the core. When these calls run on the main thread, Python signal handling, including
Ctrl-C/KeyboardInterrupt, is deferred until control returns to Python. It does not
stop an in-progress core computation or become a KiyosiError.
"""

from typing import Literal

from ._native import ErrorCategory, KiyosiError, PricingResult, __version__

Greek = Literal[
    "delta",
    "gamma",
    "speed",
    "theta",
    "charm",
    "color",
    "vega",
    "vanna",
    "zomma",
    "rho",
]

__all__ = [
    "ErrorCategory",
    "Greek",
    "KiyosiError",
    "PricingResult",
    "__version__",
]
