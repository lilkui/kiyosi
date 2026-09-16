#include <kiyosi/pricing/engines/vanilla/analytic.hpp>

#include "../../detail/black_scholes.hpp"

namespace kiyosi {
using namespace detail;

result<PricingResult> AnalyticVanillaEngine::price_impl(
    const EuropeanOption& option, const PricingContext& context) const
{
    return price_at_volatility(option, context, context.parameters().volatility());
}

} // namespace kiyosi
