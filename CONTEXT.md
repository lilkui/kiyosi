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

**Asian option**:
An exercise-based option whose payoff depends on an average of underlying observations over an averaging schedule.
_Avoid_: average product

**Accumulator**:
A scheduled structured product that accumulates conditional underlying exposure or cash settlement across observation dates.
_Avoid_: averaging option

**Autocallable**:
A structured product with scheduled observations, conditional coupons or redemption, and automatic state transitions or termination.
_Avoid_: callable option

**Payoff**:
The contractual function that maps an underlying observation at a payment or exercise time to an outcome.
_Avoid_: payout rule when referring to the complete instrument

**Exercise**:
The contractual set of times at which a holder may choose to exercise an instrument.
Automatic redemption or termination caused by an observation is not exercise.

**Exercise-based option**:
An instrument whose value is determined by a payoff at one contractual time or at a holder-selected admissible time.
Vanilla, digital, and Bermudan options belong to this family.

**Trading calendar**:
Rules that identify valid trading and observation dates for an underlying market.
_Avoid_: business calendar

**Observation date**:
A contractual date on which an instrument checks the underlying or settles a scheduled condition.
_Avoid_: sample date

**Effective date**:
The first date on which an instrument's contractual terms are in force and valuation is admissible.
_Avoid_: start date, inception date

**Scheduled monitoring**:
Barrier monitoring performed only on an instrument's observation dates.
_Avoid_: discrete monitoring when referring to the BGK approximation

**Average observation interval**:
The average elapsed time between an instrument's scheduled observation dates, used to parameterize an approximate scheduled-monitoring valuation.
_Avoid_: observation frequency

**Terminal settlement**:
The contractual amount and timing due when an instrument reaches its expiry without an earlier termination.
_Avoid_: final payoff when referring to the complete settlement outcome

**Principal-inclusive valuation**:
A structured product valuation that includes the instrument's returned principal together with coupons and loss or redemption amounts.
_Avoid_: net premium valuation

**Barrier case matrix**:
The complete combination of barrier direction, cash or asset settlement, option-type condition, and settlement timing supported by a binary-barrier instrument.
_Avoid_: barrier variants

**Observation event**:
A scheduled contractual check that is applied once at its observation date and is not replayed when valuation occurs later.
_Avoid_: future observation for a date already reached

**Reference fixture**:
A checked-in input and result case derived from a pinned external implementation and used by executable parity tests.
_Avoid_: placeholder row, expected answer

**Finite-difference grid**:
The caller-selected asset and time resolution used by a discretized pricing engine, including its time-stepping scheme.
_Avoid_: recursion depth

**One-touch**:
A binary-barrier instrument with no strike condition that settles when an in-barrier is reached.
_Avoid_: barrier call

**No-touch**:
A binary-barrier instrument with no strike condition that settles when an out-barrier remains unbreached through expiry.
_Avoid_: barrier put

**Coupon accrual**:
The product-specific time convention that converts a structured product's annualized coupon rate into an event settlement amount.
_Avoid_: generic coupon scaling

**Pinned reference revision**:
The fixed external implementation revision from which a reference fixture's behavior and numerical result are derived.
_Avoid_: latest upstream result

**Behavioral parity**:
Agreement with the pinned reference implementation's caller-visible contractual and valuation behavior without source, API, or inheritance compatibility.
_Avoid_: source parity, API parity

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

**Parity exception**:
A documented caller-visible behavior that intentionally differs from the pinned reference while preserving the project's contractual or numerical correctness.
_Avoid_: parity bug

**Valuation-date observation**:
An observation event whose contractual date is the valuation date and which is applied once before any terminal settlement for that date.
_Avoid_: historical observation

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
