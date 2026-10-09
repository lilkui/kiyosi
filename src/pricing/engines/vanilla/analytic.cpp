#include <kiyosi/pricing/engines/vanilla/analytic.hpp>

#include <kiyosi/pricing/numerical_greeks.hpp>

#include "../../detail/black_scholes.hpp"

namespace kiyosi {
using namespace detail;

Result<PricingResult> AnalyticVanillaEngine::price_native(
    const EuropeanOption& option, const PricingContext& context, GreeksRequest output) const
{
    return price_at_volatility(option, context, context.model_parameters().volatility(), output);
}

Result<double> AnalyticVanillaEngine::price(
    const EuropeanOption& option, const PricingContext& context) const
{
    return detail::price_value(price_native(option, context, GreeksRequest{}));
}

Result<PricingResult> AnalyticVanillaEngine::price_with_greeks(
    const EuropeanOption& option, const PricingContext& context,
    GreeksRequest greeks, NumericalShiftSettings settings) const
{
    return detail::price_with_greeks(*this, option, context, greeks, settings, [&](const auto& engine) { return engine.price_native(option, context, greeks); }, /* native_complete: missing values are unsupported analytic limits */ true);
}

} // namespace kiyosi
