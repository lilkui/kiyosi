#pragma once

#include <kiyosi/instruments/vanilla.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>

namespace kiyosi {
class IntegralEuropeanEngine {
public:
    [[nodiscard]] result<PricingResult> price(const EuropeanOption&, const PricingContext&) const;
};
class BjerksundStenslandAmericanEngine {
public:
    [[nodiscard]] result<PricingResult> price(const AmericanOption&, const PricingContext&) const;
};
} // namespace kiyosi
