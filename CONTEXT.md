# Derivatives Pricing Domain

This context covers the definition and valuation of single-underlying derivative contracts, including their contractual schedules and numerical results.

## Language

**Instrument**:
A contractual derivative definition whose terms determine its payoff, exercise, observation, and settlement behavior.
_Avoid_: product, trade

**Pricing engine**:
A valuation method that turns an instrument and market assumptions into a numerical result.
_Avoid_: model, calculator

**Underlying**:
The asset whose observed value influences an instrument's payoff.
_Avoid_: underlying instrument

**Structured product**:
A derivative whose payoff depends on multiple contractual observations, state transitions, or path-dependent conditions.
_Avoid_: exotic

**Trading calendar**:
Rules that identify valid trading and observation dates for an underlying market.
_Avoid_: business calendar

**Observation date**:
A contractual date on which an instrument checks the underlying or settles a scheduled condition.
_Avoid_: sample date

**Valuation**:
The determination of an instrument's value and risk measures under stated market assumptions and a valuation date.
_Avoid_: calculation, quote

**Market assumptions**:
The economic inputs used for valuation, including the underlying price, volatility, rates, and dividends.
_Avoid_: configuration

**Reference result**:
A trusted valuation or risk result used to assess numerical parity for the same instrument and assumptions.
_Avoid_: expected answer

**Numerical parity**:
Agreement with an established reference result within an explicitly chosen tolerance for the same instrument and assumptions.
_Avoid_: exact match

**Rewrite**:
A redesign of the library's domain model and public contract rather than a source-level port. The redesigned library remains accountable for established financial behavior through numerical parity.
_Avoid_: port, translation
