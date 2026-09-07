# Align structured state, settlement, and validation

Type: task
Status: ready-for-agent
Blocked by: 01

## Problem

Structured constructors are public and bypass factory validation. Monte Carlo
and the current recursion mishandle known up-touch state, equality at knock-in,
past and expiry observations, and product-specific coupon accrual. Accumulator
instances can reach pricing without revalidation.

## Work

- Make `Accumulator`, `PhoenixOption`, `SnowballOption`,
  `BinarySnowballOption`, and `TernarySnowballOption` constructors private and
  make the existing `make_*` functions their only construction boundary.
- Route immutable coupon replacements through factories and return `result<T>`.
- Accept all finite signed coupon, maturity-coupon, and minimal-coupon rates;
  preserve non-negative validation for principal, barriers, and other
  contractual terms.
- Return `invalid_parameter` for numeric defects and `invalid_schedule` for
  invalid or unordered instrument-life and observation dates.
- Factor only small product-specific payoff/transition helpers shared by Monte
  Carlo and finite-difference work; do not add a generic state-machine layer.
- Keep values principal-inclusive according to `principal_ratio`.
- Return zero for already up-touched autocallables and preserve down-touch as
  known knock-in state.
- Use strict `<` for knock-in and `>=` for knock-out.
- Do not replay observation dates before or equal to valuation. At expiry,
  process an expiry observation before terminal settlement.
- Keep Phoenix event coupons fixed at `initial_price * coupon_rate`.
- Accrue Snowball, BinarySnowball, and TernarySnowball annualized coupons using
  Actual/365 Fixed from effective through the payment event.
- Discount from valuation to payment using Actual/365 Fixed. Use the calendar
  only to select and simulate trading/observation dates.
- Preserve automatic daily trading-date simulation in structured Monte Carlo;
  do not add a configurable time-step setting.
- Add product-level cases for coupon accrual, daily/expiry knock-in, pre-touched
  status, early redemption, loss floors/caps, final knock-out, and terminal
  settlement for all five instruments.
- Replace every touched structured Monte Carlo fixture row with a reviewed
  pinned reference and explicit statistical tolerance.

## Acceptance

- Invalid structured values cannot be directly constructed, including invalid
  `Accumulator` values and invalid immutable coupon replacements.
- Signed finite coupon rates are accepted; non-finite values fail.
- Every structured product has executable tests for expiry settlement and at
  least one earlier observation/state transition.
- An expiry knock-out is applied before the maturity branch.
- Past observations are not replayed in mid-life valuation.
- Up-touch, down-touch, and no-touch state produce the pinned behavior.
- Monte Carlo runs repeat for the same seed and use the derived future trading
  grid.
- Focused tests and the full CTest target pass in the Visual Studio Developer
  environment.

## Comments
