#pragma once

#include <kiyosi/instruments/asian.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>

namespace kiyosi {

/// Closed-form geometric average-rate option under lognormal spot.
class KIYOSI_EXPORT GeometricAverageAsianEngine {
public:
    [[nodiscard]] result<PricingResult> price(const GeometricAverageOption&, const PricingContext&) const;
};

/// Turnbull-Wakeman moment-matched approximation for arithmetic averaging.
class KIYOSI_EXPORT ArithmeticAverageAsianEngine {
public:
    [[nodiscard]] result<PricingResult> price(const ArithmeticAverageOption&, const PricingContext&) const;
};

} // namespace kiyosi
