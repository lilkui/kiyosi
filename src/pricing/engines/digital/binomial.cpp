#include <kiyosi/pricing/engines/digital/binomial.hpp>

#include "../../detail/numerical_greeks.hpp"
#include "../binomial.hpp"

namespace kiyosi {

Result<double> CoxRossRubinsteinDigitalEngine::price(const CashOrNothingOption& option, const PricingContext& context) const
{
    if (auto valid = detail::require_flat_rates(context); !valid) return std::unexpected(valid.error());
    return detail::price_value(detail::price_binomial(option, context, settings_, {}));
}

Result<double> CoxRossRubinsteinDigitalEngine::price(const AssetOrNothingOption& option, const PricingContext& context) const
{
    if (auto valid = detail::require_flat_rates(context); !valid) return std::unexpected(valid.error());
    return detail::price_value(detail::price_binomial(option, context, settings_, {}));
}

Result<PricingResult> CoxRossRubinsteinDigitalEngine::price_with_greeks(
    const CashOrNothingOption& option, const PricingContext& context, GreeksRequest greeks, NumericalShiftSettings settings) const
{
    if (auto valid = detail::require_flat_rates(context); !valid) return std::unexpected(valid.error());
    return detail::evaluate_greeks(*this, option, context, greeks, settings, [&](const auto& engine) {
        return detail::price_binomial(option, context, engine.settings(), greeks);
    });
}

Result<PricingResult> CoxRossRubinsteinDigitalEngine::price_with_greeks(
    const AssetOrNothingOption& option, const PricingContext& context, GreeksRequest greeks, NumericalShiftSettings settings) const
{
    if (auto valid = detail::require_flat_rates(context); !valid) return std::unexpected(valid.error());
    return detail::evaluate_greeks(*this, option, context, greeks, settings, [&](const auto& engine) {
        return detail::price_binomial(option, context, engine.settings(), greeks);
    });
}

} // namespace kiyosi
