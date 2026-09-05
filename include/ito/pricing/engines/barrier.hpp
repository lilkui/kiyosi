#pragma once

#include <ito/pricing/result.hpp>
#include <ito/market/context.hpp>
#include <ito/instruments/barrier.hpp>

namespace ito {

class AnalyticBarrierEngine {
public:
    static constexpr risk_measure_set supported_risk_measures = risk_bit(risk_measure::price);

    [[nodiscard]] result<PricingResult> price(const BarrierOption&, const PricingContext&) const;
    [[nodiscard]] result<PricingResult> price(const BarrierOption&, const PricingContext&, PricingRequest) const;
};

} // namespace ito

