#pragma once

#include <kiyosi/instruments/vanilla.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>

namespace kiyosi {

/// Simpson quadrature over the terminal lognormal density.
class KIYOSI_EXPORT IntegralEuropeanEngine {
public:
    [[nodiscard]] result<PricingResult> price(const EuropeanOption&, const PricingContext&) const;
};

} // namespace kiyosi
