#include <kiyosi/pricing/engines/vanilla/binomial.hpp>

#include "../../detail/numerical_greeks.hpp"
#include "../binomial.hpp"

namespace kiyosi {
using namespace detail;

Result<double> CoxRossRubinsteinVanillaEngine::price(const EuropeanOption& option, const PricingContext& context) const
{
    return detail::price_value(price_binomial(option, context, settings_, GreeksRequest{}));
}

Result<double> CoxRossRubinsteinVanillaEngine::price(const AmericanOption& option, const PricingContext& context) const
{
    return detail::price_value(price_binomial(option, context, settings_, GreeksRequest{}));
}

Result<PricingResult> CoxRossRubinsteinVanillaEngine::price_with_greeks(const EuropeanOption& option, const PricingContext& context,
                                                                        GreeksRequest greeks, NumericalShiftSettings settings) const
{
    return detail::price_with_greeks(*this, option, context, greeks, settings, [&](const auto& engine, const PricingContext& shifted, GreeksRequest request) { return price_binomial(option, shifted, engine.settings(), request); });
}

Result<PricingResult> CoxRossRubinsteinVanillaEngine::price_with_greeks(const AmericanOption& option, const PricingContext& context,
                                                                        GreeksRequest greeks, NumericalShiftSettings settings) const
{
    return detail::price_with_greeks(*this, option, context, greeks, settings, [&](const auto& engine, const PricingContext& shifted, GreeksRequest request) { return price_binomial(option, shifted, engine.settings(), request); });
}

} // namespace kiyosi
