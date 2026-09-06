# Organize the public API by vertical slices

The prototype is organized by cohesive domain slices inspired by QuantLib's directory layout. Core date/error types, market context and calendars, instruments, and pricing engine families each have their own public headers and implementation ownership. `kiyosi/kiyosi.hpp` remains a convenience umbrella, but slice headers are the canonical dependency boundary.

The split keeps numerical helpers private to pricing implementations and avoids introducing a public abstraction for each internal algorithm. Public names remain unchanged because this refactor improves ownership and discoverability rather than changing valuation behavior.
