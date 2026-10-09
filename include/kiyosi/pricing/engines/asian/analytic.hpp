#pragma once

#include <kiyosi/instruments/asian.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>
#include <kiyosi/pricing/settings/numerical_shift.hpp>

namespace kiyosi {

/// Closed-form continuous geometric average-price option under lognormal spot.
class KIYOSI_EXPORT AnalyticGeometricAveragePriceEngine {
public:
    /// Prices a geometric-average option. Once averaging has begun, its realized average must be positive.
    /// Before averaging begins, the realized average must be zero.
    /// @return Price, or a contract or context error.
    [[nodiscard]] Result<double> price(const GeometricAveragePriceOption& option, const PricingContext& context) const;

    /// Prices with the explicitly requested Greeks; unavailable measures remain empty.
    [[nodiscard]] Result<PricingResult> price_with_greeks(const GeometricAveragePriceOption& option, const PricingContext& context,
                                                          GreeksRequest greeks, NumericalShiftSettings settings = {}) const;

private:
    [[nodiscard]] Result<PricingResult> price_native(const GeometricAveragePriceOption& option, const PricingContext& context) const;
};

/// Turnbull-Wakeman moment-matched approximation for arithmetic averaging.
class KIYOSI_EXPORT TurnbullWakemanArithmeticAveragePriceEngine {
public:
    /// Prices an arithmetic-average option using moment matching. For a nonzero averaging window,
    /// the realized average must be zero before averaging begins and positive afterward. A zero-length
    /// window is a single fixing at expiry and follows European vanilla pricing before expiry.
    /// @return Price, or a contract or context error.
    [[nodiscard]] Result<double> price(const ArithmeticAveragePriceOption& option, const PricingContext& context) const;

    /// Prices with the explicitly requested Greeks; unavailable measures remain empty.
    [[nodiscard]] Result<PricingResult> price_with_greeks(const ArithmeticAveragePriceOption& option, const PricingContext& context,
                                                          GreeksRequest greeks, NumericalShiftSettings settings = {}) const;

private:
    [[nodiscard]] Result<PricingResult> price_native(const ArithmeticAveragePriceOption& option, const PricingContext& context) const;
};

} // namespace kiyosi
