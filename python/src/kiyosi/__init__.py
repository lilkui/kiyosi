"""Price derivatives with Rust models and engines.

Use ``kiyosi.instruments`` for contracts, ``kiyosi.market`` for market data and
calendars, and ``kiyosi.pricing`` for engines and implied-parameter solvers.

Notes
-----
Times require timezone-free ``datetime.datetime`` values with microsecond
precision and a consistent caller-selected time basis. Calendar dates require
``datetime.date`` (excluding datetimes); observation clocks require timezone-free
``datetime.time``. Results outside Python's years 1 through 9999 raise
``ValueError``.

Numeric inputs reject booleans and implicit rounding. Exact real values must
be exactly representable as floats; use ``float(value)`` to opt into rounding.
Integer inputs require ``__index__``; fractional values raise ``TypeError``
and out-of-range values raise ``OverflowError``. Decimal conversion preserves
the caller's context and flags; finite decimal overflow raises ``OverflowError``.

Domain failures raise ``KiyosiError`` subclasses. ``InvalidInputError`` also inherits
``ValueError``; representation and signature failures use native Python
exceptions, and operational failures use ``OSError``. Catch exception types;
messages are diagnostic text. ``MissingHistoryError`` identifies absent required
history and inherits ``InvalidInputError``.
Rust panics are not domain errors, and allocation
exhaustion may terminate the process.

Values own their data and expose read-only properties. Collection properties
return owned tuples, with nested tuples for surfaces. Replacement returns a new
validated value; copying returns the same immutable value. Equality is exact;
use tolerances for numerical comparisons. Engines are unhashable. Pickle uses
validated constructors; use the same Kiyosi version in each process.

Supplied history applies to the requested valuation and describes events strictly
before it; observations exactly at valuation use the current spot once.
Bookkeeping owns event records, lifecycle and settlement progression, and
consistency with valuation time and contract terms. Callers must prevent stale
or future state from being supplied; Kiyosi does not check state age or replay
history. Missing history is unknown; explicit zero, false, untouched, and no-event
states are known history where supported. Numerical time sensitivities hold
supplied history fixed. See ``kiyosi.instruments`` for product conventions.

Shared and distinct instances support concurrent reads and pricing. Long calls
release the GIL; short analytic calls may retain it. Parallel Monte Carlo calls
share a library-owned worker pool, sized on first use, with per-call buffers and
worker caps. Set its process-wide size cap with
``kiyosi.pricing.configure_monte_carlo_pool(max_workers=...)`` before the first
parallel request. It ignores ``RAYON_NUM_THREADS``. Worker-creation failures raise
``OSError`` and initialization can be retried. Use multiprocessing's ``spawn``
start method; a pool inherited through ``fork`` is unusable.

Calls are synchronous. During long requests the calling thread polls signals.
Signal-handler exceptions, including ``KeyboardInterrupt``, cancel the worker
and propagate after it stops, without a partial result. Cancellation is
cooperative; allocation and individual work units may delay it.

"""

from . import _native
from ._native import (
    ComputationFailureError,
    Greek,
    GreekUnavailableError,
    InvalidInputError,
    KiyosiError,
    MissingHistoryError,
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
    "GreekUnavailableError",
    "InvalidInputError",
    "KiyosiError",
    "MissingHistoryError",
    "NonConvergenceError",
    "PricingResult",
    "RootNotBracketedError",
    "UnsupportedOperationError",
    "__version__",
]
