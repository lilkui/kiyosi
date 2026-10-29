"""Python API for the Rust derivatives pricing core.

Domain rejections raise a specific KiyosiError subclass: InvalidInputError,
UnsupportedOperationError, ComputationFailureError, RootNotBracketedError or
NonConvergenceError. Catch a subclass to handle one failure kind, or KiyosiError
for any domain rejection. Representation and signature errors use Python's native
exceptions. Reported operational failures retain native OSError semantics. Rust
allocation exhaustion may terminate the process; internal panics are separate
from domain errors.

Values own their data, expose read-only properties and support concurrent reads
and pricing on shared or distinct instances. Replacement returns a new value.
Long calculations release the GIL around owned Rust inputs. Short analytic calls
may retain it; see the Rust core documentation for the concurrency contract.

Calls are synchronous, without cancellation or core signal polling. Python signal
handling, including KeyboardInterrupt, resumes when control returns to Python.
"""

from ._native import (
    ComputationFailureError,
    Greek,
    InvalidInputError,
    KiyosiError,
    NonConvergenceError,
    PricingResult,
    RootNotBracketedError,
    UnsupportedOperationError,
    __version__,
)

__all__ = [
    "ComputationFailureError",
    "Greek",
    "InvalidInputError",
    "KiyosiError",
    "NonConvergenceError",
    "PricingResult",
    "RootNotBracketedError",
    "UnsupportedOperationError",
    "__version__",
]
