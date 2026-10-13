#include <kiyosi/pricing/engines/vanilla/analytic.hpp>

#include <kiyosi/pricing/numerical_greeks.hpp>

#include "../../detail/black_scholes.hpp"

namespace kiyosi {
using namespace detail;

Result<double> AnalyticVanillaEngine::price(
    const EuropeanOption& option, const PricingContext& context) const
{
    return detail::price_value(price_at_volatility(option, context, context.model_parameters().volatility(), GreeksRequest{}));
}

Result<PricingResult> AnalyticVanillaEngine::price_with_greeks(
    const EuropeanOption& option, const PricingContext& context,
    GreeksRequest greeks, NumericalShiftSettings settings) const
{
    return detail::evaluate_greeks(*this, option, context, greeks, settings, [&](const auto&) { return price_at_volatility(option, context, context.model_parameters().volatility(), greeks); });
}

} // namespace kiyosi
