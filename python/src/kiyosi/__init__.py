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
value. Long calculations release the GIL while borrowing immutable Rust
inputs for the duration of the call; short analytic calls may retain it.
Parallel Monte Carlo calls share a library-owned Rayon worker pool and keep
their own simulation buffers. The pool is sized from available CPUs on first
use and retained until process exit; it ignores `RAYON_NUM_THREADS` and leaves
the application's global Rayon pool unchanged. `thread_count` caps workers per
call without reserving them; one runs on the calling thread and zero selects
automatic sizing. Worker-creation failures raise `OSError` during pricing;
failed initialization can be retried by a later call.
For multiprocessing, use the `spawn` start method. Forking after the worker
pool has initialized leaves it unusable in the child process.

`Greek` accepts delta, gamma, speed, theta, charm, color, vega, vanna,
zomma and rho. Spot derivatives use spot units, volatility and rate
derivatives use percentage points, and time derivatives use calendar days
of forward valuation time. `PricingResult` leaves unavailable or
unrequested sensitivities as None; `require` raises if one is absent.

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
