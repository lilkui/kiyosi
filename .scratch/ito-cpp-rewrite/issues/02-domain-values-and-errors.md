Type: task
Status: ready-for-agent
Blocked by: 01

# Domain values and validated factories

## Goal

Define immutable value types for the first European-option domain, BSM parameters, pricing context, pricing result, stable errors, and free factories returning `std::expected`.

## Depends on

Build and test foundation.

## Acceptance criteria

- European call and put terms are represented by one concrete option value and an option-type enum.
- BSM parameters validate finite rates/dividends and positive finite volatility.
- Asset price, date ordering, strike, and expiry rules are validated.
- Factories return `std::expected` rather than throwing or creating invalid values.
- Errors expose a stable category and readable message.
- Pricing result fields include the agreed value and full deterministic Greek contract with documented units.
- Value types are copyable, immutable after construction, and contain no ownership hazards.

## Test

Use Catch2 to cover valid construction, every invalid boundary, error categories/messages, copy semantics, expiry, and call/put representation.
