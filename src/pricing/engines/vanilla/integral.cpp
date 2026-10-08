#include <kiyosi/pricing/engines/vanilla/integral.hpp>

#include <algorithm>
#include <cmath>

#include "../../detail/math.hpp"

namespace kiyosi {
using namespace detail;

Result<PricingResult> QuadratureVanillaEngine::price_native(const EuropeanOption& option, const PricingContext& context) const
{
    auto valid = validate_valuation_within_instrument_life(context.valuation_time(), option.effective_date(), option.expiry_date());
    if (!valid) return std::unexpected(valid.error());
    const double tau = actual_365_fixed_year_fraction(context.valuation_time(), option.expiry_date());
    const double spot = context.spot_price();
    const double strike = option.strike();
    const double sign = option.option_type() == OptionType::call ? 1.0 : -1.0;
    if (tau == 0.0)
        return make_pricing_result(std::max(sign * (spot - strike), 0.0));
    const double sigma = context.model_parameters().volatility();
    const double rate = context.model_parameters().risk_free_rate();
    const double dividend = context.model_parameters().dividend_yield();
    const double root = std::sqrt(tau);
    if (sigma < 1e-12)
        return make_pricing_result(std::exp(-rate * tau) *
                                   std::max(sign * (spot * std::exp((rate - dividend) * tau) - strike),
                                            0.0));
    const double width = sigma * root;
    const double threshold = (std::log(strike) - std::log(spot) - (rate - dividend) * tau) / width + 0.5 * width;
    if (!std::isfinite(width) || !std::isfinite(threshold))
        return std::unexpected(Error{ErrorCategory::invalid_result, "integral pricing parameters are non-finite"});
    // Completing the square centers the asset-weighted density at width instead of zero.
    const double asset_probability = normal_tail_integral(sign * (threshold - width));
    const double cash_probability = normal_tail_integral(sign * threshold);
    const double value = sign * (spot * std::exp(-dividend * tau) * asset_probability -
                                 strike * std::exp(-rate * tau) * cash_probability);
    if (!std::isfinite(value)) return std::unexpected(Error{ErrorCategory::invalid_result, "integral pricing produced a non-finite result"});
    return make_pricing_result(value);
}

} // namespace kiyosi
