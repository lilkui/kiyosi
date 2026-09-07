# Add effective dates and contractual calendar semantics

Type: task
Status: ready-for-agent

## Problem

Exercise-based, digital, Asian, and barrier instruments do not consistently
retain their effective dates. Several overloads accept a valuation date only
to discard it. Schedule factories emit the effective date itself, retain a
five-day following bound, and reject fixed-interval dates that collapse after
adjustment. SSE data covers only 2024--2026 and uses 252 trading days per year.

## Work

- Add and expose effective dates on `OptionTerms`, `AsianOptionTerms`,
  `BarrierOption`, and `BinaryBarrierOption`.
- Make the canonical factory order `... effective, expiry`; remove expiry-only
  and valuation-date compatibility overloads throughout vanilla, digital,
  Asian, vanilla barrier, and binary-barrier APIs.
- Require an inclusive valid life interval and reject pricing before effective
  or after expiry in every affected pricing engine.
- Keep effective dates independent of trading-day membership.
- Require Asian averaging start within the instrument life.
- Change fixed-interval schedules to start at `effective + interval`, adjust
  following through expiry, de-duplicate collapsed adjusted dates, and return
  an empty schedule when the interval exceeds the term.
- Add positive `lock_up_months` to monthly schedules, begin at that month
  offset, continue monthly, and return empty when the lock-up exceeds the term.
- Remove `following_bound` from schedule APIs.
- Port the pinned DerivaSharp 1991--2030 SSE holiday table and set the annual
  trading-day convention to 243. Outside that range, weekdays remain open and
  weekends remain closed.
- Keep contractual schedule ordering/life validation at construction and
  trading-day validation at pricing against the context calendar.
- Update all call sites and affected reference rows; do not preserve placeholder
  values for engines changed by this issue.

## Acceptance

- Factories cannot create an instrument with effective after expiry or Asian
  averaging outside its life.
- Every affected pricing engine accepts valuation on effective and expiry, and
  rejects valuation outside that inclusive range.
- No public expiry-only or discarded-valuation overload remains.
- Fixed schedules skip effective, de-duplicate following-adjustment collisions,
  stop at expiry, and support an empty result.
- Monthly schedules honor positive lock-up months and following adjustment at
  weekend/holiday and expiry boundaries.
- SSE regression cases cover holidays before 2024 and after 2026, the 243-day
  denominator, weekends, and a weekday outside the pinned holiday range.
- Public-header smoke tests and all existing engine tests compile with the new
  signatures.
- Focused tests and the full CTest target pass in the Visual Studio Developer
  environment.

## Comments
