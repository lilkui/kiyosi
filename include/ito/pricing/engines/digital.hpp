#pragma once

#include <ito/pricing/result.hpp>
#include <ito/market/context.hpp>
#include <ito/instruments/digital.hpp>

namespace ito {

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

} // namespace ito

