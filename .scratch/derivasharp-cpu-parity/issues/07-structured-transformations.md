# 07: Add immutable structured transformations

**What to build:** Public immutable transformations for structured instruments so callers can replace historical touch status on all four autocallable notes and replace a Snowball's complete knock-out coupon schedule together with its maturity coupon.

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [ ] Phoenix, Snowball, BinarySnowball, and TernarySnowball expose validated touch-status replacement.
- [ ] Snowball exposes validated replacement of the full knock-out coupon schedule and maturity coupon together.
- [ ] Each replacement returns `result<T>` and reuses existing instrument validation.
- [ ] Invalid touch values, schedule lengths or ordering, and non-finite coupon values are rejected.
- [ ] The original instrument remains unchanged after every successful or failed replacement.
- [ ] Generic factories continue to express all required presets without adding named convenience factories.
