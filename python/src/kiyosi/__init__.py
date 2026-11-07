"""Price derivatives with Rust pricing models and engines.

Notes
-----
Domain failures raise a specific `KiyosiError` subclass:
`InvalidInputError`, `UnsupportedOperationError`, `ComputationFailureError`,
`RootNotBracketedError` or `NonConvergenceError`. Catch a subclass for one
failure category or `KiyosiError` for any domain failure. `InvalidInputError`
also inherits `ValueError`. Validation
diagnostics include field names; messages are for humans, not parsing. Representation
and signature errors use native Python exceptions; reported operational
failures use `OSError`. Internal Rust panics are separate from domain
exceptions; allocation exhaustion may terminate the process.

Contract, valuation, curve and observation times use ``datetime.datetime``
with ``tzinfo=None`` and microsecond precision. Plain dates and datetimes with
timezone information raise ``TypeError`` for time parameters. Calendar inputs
require ``datetime.date``, excluding datetime instances. Callers must supply a
consistent time basis. Automatic schedules use an explicit ``datetime.time``
observation clock with the same timezone and precision rules. Python dates and
datetimes support years 1 through 9999.
The Rust core has a wider date range; converting a result outside Python's range
raises ``ValueError``, including calendar adjustments past either Python limit.
Numeric inputs reject booleans and unintended lossy conversions. Integers,
Decimal values and other exact real numbers must be representable exactly as a
float; use an explicit ``float(value)`` conversion to opt into rounding.
This also applies to integer scalars from libraries such as NumPy.
Finite Decimal values outside the float range raise ``OverflowError``. Conversion
preserves the caller's decimal context and flags.
Integer inputs accept the ``__index__`` protocol, excluding booleans; fractional
values raise ``TypeError`` and values outside the target integer range raise
``OverflowError``.

Values own their data and expose read-only properties. Default representations
show contract terms, market inputs, engine controls and result values, with
large arrays abbreviated through bounded native previews for interactive inspection.
Representations do not copy full surfaces, curves or observation arrays. Shared and distinct
instances support concurrent reads and pricing. Replacement creates a new
value. ``copy.copy`` and ``copy.deepcopy`` return the same immutable value;
pricing results compare by value, including Greek request/support metadata and
Monte Carlo uncertainty. Engines compare by type and configuration; stateless
engines of the same type compare equal. Engines are unhashable. Equality is exact;
use tolerances for numerical comparisons.
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
zomma, rho and dividend_rho. All are unscaled partial derivatives of price V.
Spot S uses spot units, volatility σ and rates use decimal values, and forward
valuation time t uses years (Actual/365 Fixed). Other inputs, including expiry,
remain fixed. Delta, gamma and speed are ∂V/∂S, ∂²V/∂S² and ∂³V/∂S³.
Vega and volga are ∂V/∂σ and ∂²V/∂σ²; vanna and zomma are
∂²V/(∂S∂σ) and ∂³V/(∂S²∂σ). Theta, charm and color are
∂V/∂t, ∂²V/(∂S∂t) and ∂³V/(∂S²∂t), with market curves held fixed.
`PricingResult` leaves unavailable or
unrequested sensitivities as None; `require` raises if one is absent.
Use ``result.status("delta")`` to distinguish ``"available"``, ``"not_requested"``,
``"unsupported"`` (outside the engine/model coverage) and ``"undefined"``
(supported but unavailable at this valuation or numerical stencil). The
``requested`` and ``supported`` properties return owned lists of Greek names.
Rho and dividend rho are ∂V/∂r and ∂V/∂q for parallel decimal shifts of the
continuously compounded risk-free and dividend-yield curves, respectively,
holding other market inputs fixed.

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
