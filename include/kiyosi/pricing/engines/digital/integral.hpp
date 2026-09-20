#pragma once

#include <kiyosi/instruments/digital.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>

namespace kiyosi {

/// Simpson quadrature over the terminal lognormal density.
class KIYOSI_EXPORT QuadratureDigitalEngine {
public:
    [[nodiscard]] Result<PricingResult> price(const CashOrNothingOption&, const PricingContext&) const;
    [[nodiscard]] Result<PricingResult> price(const AssetOrNothingOption&, const PricingContext&) const;
};

} // namespace kiyosi
