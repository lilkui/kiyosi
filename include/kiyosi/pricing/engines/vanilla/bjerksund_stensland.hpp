#pragma once

#include <kiyosi/instruments/vanilla.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>

namespace kiyosi {

class KIYOSI_EXPORT BjerksundStenslandAmericanEngine {
public:
    [[nodiscard]] result<PricingResult> price(const AmericanOption&, const PricingContext&) const;
};

} // namespace kiyosi
