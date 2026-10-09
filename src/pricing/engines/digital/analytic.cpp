#include <kiyosi/pricing/engines/digital/analytic.hpp>

#include <cmath>
#include <optional>

#include "../../detail/math.hpp"

namespace kiyosi {
using namespace detail;

Result<PricingResult> AnalyticDigitalEngine::price_impl(
    OptionType type, double strike, double payout, bool asset_settlement, Date effective_date, Date expiry_date,
    const PricingContext& context, GreeksRequest output) const
{
    const auto valid = validate_valuation_within_instrument_life(context.valuation_time(), effective_date, expiry_date);
    if (!valid) return std::unexpected(valid.error());
    const double spot = context.spot_price();
    const double t = actual_365_fixed_year_fraction(context.valuation_time(), expiry_date);
    const double sign = type == OptionType::call ? 1.0 : -1.0;
    if (t == 0.0) {
        const bool exercised = sign * (spot - strike) > 0.0;
        return make_pricing_result(exercised ? (asset_settlement ? spot : payout) : 0.0);
    }
    const double sigma = context.model_parameters().volatility();
    const double root_t = std::sqrt(t);
    const double rate_df = std::exp(-context.model_parameters().risk_free_rate() * t);
    const double div_df = std::exp(-context.model_parameters().dividend_yield() * t);
    const double volatility_time = sigma * root_t;
    const double forward = (log_price_ratio(spot, strike) +
                            (context.model_parameters().risk_free_rate() - context.model_parameters().dividend_yield()) * t) /
                           volatility_time;
    const double d1 = forward + 0.5 * volatility_time;
    const double d2 = forward - 0.5 * volatility_time;
    const double d = asset_settlement ? d1 : d2;
    const double nd = normal_cdf(sign * d);
    const double scale = asset_settlement ? spot * div_df : payout * rate_df;
    const double value = sign * d < -10.0
                             ? exponential_normal_cdf(std::log(asset_settlement ? spot : payout) -
                                                          (asset_settlement ? context.model_parameters().dividend_yield() : context.model_parameters().risk_free_rate()) * t,
                                                      sign * d)
                             : scale * nd;
    if (!std::isfinite(value))
        return std::unexpected(Error{ErrorCategory::invalid_result, "analytic pricing produced a non-finite result"});
    if (!output.has(Greek::delta) && !output.has(Greek::gamma))
        return make_pricing_result(value);
    const double log_volatility_time = std::log(volatility_time);
    const double log_spot = std::log(spot);
    // Scale the density in log space: even an underflowed tail can have representable derivatives.
    const auto weighted_density = [&](double log_weight) {
        return std::isfinite(d) ? std::exp(log_weight - 0.5 * d * d + std::log(inverse_sqrt_two_pi)) : 0.0;
    };
    std::optional<double> delta;
    std::optional<double> gamma;
    if (asset_settlement) {
        const double log_discount = -context.model_parameters().dividend_yield() * t;
        if (output.has(Greek::delta)) delta = div_df * nd + sign * weighted_density(log_discount - log_volatility_time);
        if (output.has(Greek::gamma)) {
            const double numerator = volatility_time - d1;
            gamma = numerator == 0.0 ? 0.0 : sign * std::copysign(weighted_density(log_discount + std::log(std::abs(numerator)) - log_spot - 2.0 * log_volatility_time), numerator);
        }
    } else {
        const double log_scale = std::log(payout) - context.model_parameters().risk_free_rate() * t;
        if (output.has(Greek::delta)) delta = sign * weighted_density(log_scale - log_spot - log_volatility_time);
        if (output.has(Greek::gamma)) {
            const double numerator = volatility_time + d2;
            gamma = numerator == 0.0 ? 0.0 : -sign * std::copysign(weighted_density(log_scale + std::log(std::abs(numerator)) - 2.0 * log_spot - 2.0 * log_volatility_time), numerator);
        }
    }
    auto result = make_pricing_result(value, {{Greek::delta, delta}, {Greek::gamma, gamma}});
    if (!result) return std::unexpected(result.error());
    if (!result->all_finite())
        return std::unexpected(Error{ErrorCategory::invalid_result, "analytic pricing produced a non-finite result"});
    return result;
}

} // namespace kiyosi
