#pragma once

#include <kiyosi/instruments/vanilla.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>
#include <kiyosi/pricing/settings/numerical_shift.hpp>

namespace kiyosi {

/// Bjerksund-Stensland (2002) two-step American approximation.
/// Before expiry, calls require a non-negative risk-free rate and puts require a
/// non-negative dividend yield (the interest rate under put-call symmetry).
/// The approximation rejects market inputs whose exercise boundaries do not exceed the strike.
class KIYOSI_EXPORT BjerksundStenslandVanillaEngine {
public:
    /// Prices an American vanilla option with the two-step approximation.
    /// @return Price, a contract or context error, or `unsupported_operation` for a negative transformed
    /// rate or nonphysical exercise boundaries. Use a tree or finite-difference engine for those inputs.
    /// Returns `invalid_result` when the exercise exponent or price cannot be represented reliably.
    [[nodiscard]] Result<double> price(
        const AmericanOption& option, const PricingContext& context) const;

    /// Prices with the explicitly requested Greeks; unavailable measures remain empty.
    [[nodiscard]] Result<PricingResult> price_with_greeks(
        const AmericanOption& option, const PricingContext& context,
        GreeksRequest greeks, NumericalShiftSettings settings = {}) const;
};

} // namespace kiyosi
