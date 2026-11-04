"""Price derivatives with Rust pricing models and engines.

Notes
-----
Domain failures raise a specific `KiyosiError` subclass:
`InvalidInputError`, `UnsupportedOperationError`, `ComputationFailureError`,
`RootNotBracketedError` or `NonConvergenceError`. Catch a subclass for one
failure category or `KiyosiError` for any domain failure. Representation
and signature errors use native Python exceptions; reported operational
failures use `OSError`. Internal Rust panics are separate from domain
exceptions; allocation exhaustion may terminate the process.

Contract, valuation, curve and observation times use ``datetime.datetime``
with ``tzinfo=None`` and microsecond precision. Plain dates and datetimes with
timezone information raise ``TypeError`` for time parameters. Calendar inputs
require ``datetime.date``, excluding datetime instances. Callers must supply a
consistent time basis.
Numeric inputs reject booleans and unintended lossy conversions.

Values own their data and expose read-only properties. Shared and distinct
instances support concurrent reads and pricing. Replacement creates a new
value. ``copy.copy`` and ``copy.deepcopy`` return the same immutable value;
``pickle`` reconstructs contracts, market data, calendars, schedules, engines and
results through validated constructors. Use the same Kiyosi version in each process.
Long calculations release the GIL while borrowing immutable Rust
inputs for the duration of the call; short analytic calls may retain it.
Parallel Monte Carlo calls share a library-owned Rayon worker pool and keep
their own simulation buffers. The pool is sized from available CPUs on first
use and retained until process exit; it ignores `RAYON_NUM_THREADS` and leaves
the application's global Rayon pool unchanged. `thread_count` caps workers per
call without reserving them; one runs on the calling thread and zero selects
automatic sizing. Worker-creation failures raise `OSError` during pricing;
failed initialization can be retried by a later call.
For multiprocessing, use the `spawn` start method; library values can be passed
to workers and pricing results returned through process queues. Forking after the worker
pool has initialized leaves it unusable in the child process.

`Greek` accepts delta, gamma, speed, theta, charm, color, vega, volga, vanna,
zomma, rho and dividend_rho. Spot derivatives use spot units, volatility and rate
derivatives use percentage points (squared for volga), and time derivatives use
calendar days of forward valuation time. `PricingResult` leaves unavailable or
unrequested sensitivities as None; `require` raises if one is absent.
Volga is the second volatility derivative divided by 10000. Dividend rho is
the sensitivity to a parallel shift of the continuously compounded dividend-yield curve,
divided by 100, holding other market inputs fixed.

Calls are synchronous, without core cancellation or signal polling. Python
signal handling, including `KeyboardInterrupt`, resumes when control
returns to Python.
"""

from . import _native
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
from ._serialization import install as _install_serialization
from ._signatures import install as _install_signatures

_install_signatures(_native)
_install_serialization(_native)

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
