# 05: Process structured valuation-date and expiry events

**What to build:** A shared structured event schedule that applies contractual valuation-date observations exactly once, excludes dates before valuation, and processes expiry observations before terminal settlement across structured Monte Carlo and finite-difference valuation.

**Blocked by:** 01: Establish executable reference fixture pipeline

**Status:** ready-for-human

- [x] A valuation-date observation can apply coupon, knock-in, knock-out, or accumulator effects exactly once.
- [x] Dates before valuation are excluded from all structured event processing.
- [x] An expiry observation is processed before terminal settlement.
- [x] Repeated valuation does not replay a previously processed observation.
- [x] Pinned cases cover Snowball, BinarySnowball, TernarySnowball, Phoenix, and Accumulator behavior.
