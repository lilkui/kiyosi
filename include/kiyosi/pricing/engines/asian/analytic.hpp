#pragma once

#include <kiyosi/instruments/asian.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>

namespace kiyosi {

/// Closed-form geometric average-rate option under lognormal spot.
class KIYOSI_EXPORT AnalyticGeometricAverageAsianEngine {
public:
    [[nodiscard]] Result<PricingResult> price(const GeometricAveragePriceOption&, const PricingContext&) const;
};

/// Turnbull-Wakeman moment-matched approximation for arithmetic averaging.
class KIYOSI_EXPORT TurnbullWakemanArithmeticAverageAsianEngine {
public:
    [[nodiscard]] Result<PricingResult> price(const ArithmeticAveragePriceOption&, const PricingContext&) const;
};

} // namespace kiyosi
