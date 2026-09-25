#include <kiyosi/pricing/engines/vanilla/analytic.hpp>

#include "../../detail/black_scholes.hpp"

namespace kiyosi {
using namespace detail;

Result<PricingResult> AnalyticVanillaEngine::price_impl(
    const EuropeanOption& option, const PricingContext& context, detail::RiskMeasureOutput output) const
{
    return price_at_volatility(option, context, context.model_parameters().volatility(), output);
}

} // namespace kiyosi
