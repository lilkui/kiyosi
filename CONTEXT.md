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

**Payoff**:
The contractual function that maps an underlying observation at a payment or exercise time to an outcome.
_Avoid_: payout rule when referring to the complete instrument

**Exercise**:
The contractual set of times at which a holder may choose to exercise an instrument.
Automatic redemption or termination caused by an observation is not exercise.

**Exercise-based option**:
An instrument whose value is determined by a payoff at one contractual time or at a holder-selected admissible time.
Vanilla, digital, and Bermudan options belong to this family.

**Autocallable**:
A structured product with scheduled observations, conditional coupons or redemption, and automatic state transitions or termination.
It is not an exercise-based option.

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

**Risk measure**:
A valuation result or sensitivity reported for an instrument, including price and the named Greeks.
_Avoid_: metric

**Validation property**:
A caller-visible numerical relationship, bound, or monotonic behavior that a pricing result must satisfy.
_Avoid_: implementation invariant

**Convergence**:
The approach of a discretized pricing result toward a reference result as its numerical resolution increases.
_Avoid_: precision

**Rewrite**:
A redesign of the library's domain model and public contract rather than a source-level port. The redesigned library remains accountable for established financial behavior through numerical parity.
_Avoid_: port, translation
