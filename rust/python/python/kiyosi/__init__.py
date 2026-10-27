"""Isolated Rust candidate. See docs/rust-candidate.md for scope and contracts."""

from ._native import ErrorCategory, KiyosiError, __version__

__all__ = ["ErrorCategory", "KiyosiError", "__version__"]
