#include <kiyosi/pricing/engines/vanilla/integral.hpp>

#include <kiyosi/pricing/numerical_greeks.hpp>

#include <algorithm>
#include <cmath>

#include "../../detail/black_scholes.hpp"
#include "../../detail/math.hpp"

namespace kiyosi {
using namespace detail;

Result<double> QuadratureVanillaEngine::price(const EuropeanOption& option, const PricingContext& context) const
{
    auto valid = validate_valuation_within_instrument_life(context.valuation_time(), option.effective_date(), option.expiry_date());
    if (!valid) return std::unexpected(valid.error());
    const double tau = actual_365_fixed_year_fraction(context.valuation_time(), option.expiry_date());
    const double spot = context.spot_price();
    const double strike = option.strike();
    const double sign = option.option_type() == OptionType::call ? 1.0 : -1.0;
    if (tau == 0.0)
        return checked_price(std::max(sign * (spot - strike), 0.0));
    const double sigma = context.model_parameters().volatility();
    const double rate = context.model_parameters().risk_free_rate();
    const double dividend = context.model_parameters().dividend_yield();
    const double root = std::sqrt(tau);
    const double width = sigma * root;
    // Separate tail integrals lose time value when their difference is tiny.
    if (width < 1e-5)
        return detail::price_value(price_at_volatility(option, context, sigma, GreeksRequest{}));
    const double threshold = (std::log(strike) - std::log(spot) - (rate - dividend) * tau) / width + 0.5 * width;
    if (!std::isfinite(width) || !std::isfinite(threshold))
        return std::unexpected(Error{ErrorCategory::invalid_result, "integral pricing parameters are non-finite"});
    // Completing the square centers the asset-weighted density at width instead of zero.
    const double asset_value = normal_tail_integral(sign * (threshold - width), spot, -dividend * tau);
    const double cash_value = normal_tail_integral(sign * threshold, strike, -rate * tau);
    const double value = sign * (asset_value - cash_value);
    if (!std::isfinite(value)) return std::unexpected(Error{ErrorCategory::invalid_result, "integral pricing produced a non-finite result"});
    return value;
}

Result<PricingResult> QuadratureVanillaEngine::price_with_greeks(
    const EuropeanOption& option, const PricingContext& context,
    GreeksRequest greeks, NumericalShiftSettings settings) const
{
    return detail::price_with_greeks(*this, option, context, greeks, settings);
}

} // namespace kiyosi
