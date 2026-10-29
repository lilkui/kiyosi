"""Price derivatives with validated Rust contracts and engines.

Notes
-----
Domain failures raise a specific `KiyosiError` subclass:
`InvalidInputError`, `UnsupportedOperationError`, `ComputationFailureError`,
`RootNotBracketedError` or `NonConvergenceError`. Catch a subclass for one
failure category or `KiyosiError` for any domain failure. Representation
and signature errors use native Python exceptions; reported operational
failures use `OSError`. Internal Rust panics are separate from domain
exceptions; allocation exhaustion may terminate the process.

Contract and calendar dates require `datetime.date`, excluding datetime
instances. Instant inputs accept dates at midnight UTC or timezone-aware
datetimes normalized to UTC; naive datetimes are rejected. Numeric inputs
reject booleans and unintended lossy conversions.

Values own their data and expose read-only properties. Shared and distinct
instances support concurrent reads and pricing. Replacement creates a new
value. Long calculations release the GIL around owned Rust inputs; short
analytic calls may retain it.

Calls are synchronous, without core cancellation or signal polling. Python
signal handling, including `KeyboardInterrupt`, resumes when control
returns to Python.
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
