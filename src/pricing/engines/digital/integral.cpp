#include <kiyosi/pricing/engines/digital/integral.hpp>

#include <algorithm>
#include <cmath>

#include "../../detail/math.hpp"

namespace kiyosi {
using namespace detail;

namespace {
Result<PricingResult> price_digital_integral(OptionType type, double strike, double payout, bool asset,
                                             Date effective_date, Date expiry_date, const PricingContext& context)
{
    const auto valid = validate_valuation_within_instrument_life(context.valuation_time(), effective_date, expiry_date);
    if (!valid) return std::unexpected(valid.error());
    const double time = actual_365_fixed_year_fraction(context.valuation_time(), expiry_date);
    const double spot = context.spot_price();
    const double sign = type == OptionType::call ? 1.0 : -1.0;
    if (time == 0.0)
        return make_pricing_result(sign * (spot - strike) > 0.0 ? (asset ? spot : payout) : 0.0);
    const double rate = context.model_parameters().risk_free_rate();
    const double dividend = context.model_parameters().dividend_yield();
    const double volatility = context.model_parameters().volatility();
    const double root = std::sqrt(time);
    const double width = volatility * root;
    const double threshold = (std::log(strike) - std::log(spot) - (rate - dividend) * time) / width + 0.5 * width;
    if (!std::isfinite(width) || !std::isfinite(threshold))
        return std::unexpected(Error{ErrorCategory::invalid_result, "integral pricing parameters are non-finite"});
    const double probability = normal_tail_integral(sign * (threshold - (asset ? width : 0.0)));
    const double value = (asset ? spot * std::exp(-dividend * time) : payout * std::exp(-rate * time)) * probability;
    if (!std::isfinite(value)) return std::unexpected(Error{ErrorCategory::invalid_result, "integral pricing produced a non-finite result"});
    return make_pricing_result(value);
}
} // namespace

Result<PricingResult> QuadratureDigitalEngine::price_native(const CashOrNothingOption& option, const PricingContext& context) const
{
    return price_digital_integral(option.option_type(), option.strike(), option.payout(), false, option.effective_date(), option.expiry_date(), context);
}
Result<PricingResult> QuadratureDigitalEngine::price_native(const AssetOrNothingOption& option, const PricingContext& context) const
{
    return price_digital_integral(option.option_type(), option.strike(), 1.0, true, option.effective_date(), option.expiry_date(), context);
}

} // namespace kiyosi
