#pragma once

#include <kiyosi/pricing/result.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/instruments/barrier.hpp>

namespace kiyosi {

class AnalyticBarrierEngine {
public:
    static constexpr risk_measure_set supported_risk_measures = risk_bit(risk_measure::price);

    [[nodiscard]] result<PricingResult> price(const BarrierOption&, const PricingContext&) const;
    [[nodiscard]] result<PricingResult> price(const BarrierOption&, const PricingContext&, PricingRequest) const;
};

} // namespace kiyosi

