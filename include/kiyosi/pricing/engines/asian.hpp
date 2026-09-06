#pragma once

#include <kiyosi/instruments/asian.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>

namespace kiyosi {
class GeometricAverageAsianEngine {
public:
    [[nodiscard]] result<PricingResult> price(const GeometricAverageOption&, const PricingContext&) const;
};
class ArithmeticAverageAsianEngine {
public:
    [[nodiscard]] result<PricingResult> price(const ArithmeticAverageOption&, const PricingContext&) const;
};
} // namespace kiyosi
