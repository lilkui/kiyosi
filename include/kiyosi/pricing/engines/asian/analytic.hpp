#pragma once

#include <kiyosi/instruments/asian.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>

namespace kiyosi {

/// Closed-form geometric average-rate option under lognormal spot.
class KIYOSI_EXPORT AnalyticGeometricAveragePriceEngine {
public:
    /// Prices a geometric-average option.
    /// @return Pricing measures, or a contract or context error.
    [[nodiscard]] Result<PricingResult> price(const GeometricAveragePriceOption&, const PricingContext&) const;
};

/// Turnbull-Wakeman moment-matched approximation for arithmetic averaging.
class KIYOSI_EXPORT TurnbullWakemanArithmeticAveragePriceEngine {
public:
    /// Prices an arithmetic-average option using moment matching.
    /// @return Pricing measures, or a contract or context error.
    [[nodiscard]] Result<PricingResult> price(const ArithmeticAveragePriceOption&, const PricingContext&) const;
};

} // namespace kiyosi
