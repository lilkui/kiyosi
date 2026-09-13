#pragma once

#include <kiyosi/instruments/vanilla.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>

namespace kiyosi {

/// Bjerksund-Stensland (2002) two-step American approximation.
class KIYOSI_EXPORT BjerksundStenslandAmericanEngine {
public:
    [[nodiscard]] result<PricingResult> price(const AmericanOption&, const PricingContext&) const;
};

} // namespace kiyosi
