#pragma once

#include <kiyosi/instruments/digital.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>

namespace kiyosi {

class KIYOSI_EXPORT IntegralDigitalEngine {
public:
    [[nodiscard]] result<PricingResult> price(const EuropeanCashOrNothingOption&, const PricingContext&) const;
    [[nodiscard]] result<PricingResult> price(const EuropeanAssetOrNothingOption&, const PricingContext&) const;
};

} // namespace kiyosi
