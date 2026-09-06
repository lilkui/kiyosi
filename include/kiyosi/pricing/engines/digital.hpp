#pragma once

#include <kiyosi/pricing/result.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/instruments/digital.hpp>

namespace kiyosi {

class AnalyticDigitalEngine {
public:
    static constexpr risk_measure_set supported_risk_measures =
        risk_bit(risk_measure::price) | risk_bit(risk_measure::delta) | risk_bit(risk_measure::gamma);

    [[nodiscard]] result<PricingResult> price(const CashOrNothingOption&, const PricingContext&) const;
    [[nodiscard]] result<PricingResult> price(const AssetOrNothingOption&, const PricingContext&) const;
    [[nodiscard]] result<PricingResult> price(
        const CashOrNothingOption&, const PricingContext&, PricingRequest) const;
    [[nodiscard]] result<PricingResult> price(
        const AssetOrNothingOption&, const PricingContext&, PricingRequest) const;
};

} // namespace kiyosi

