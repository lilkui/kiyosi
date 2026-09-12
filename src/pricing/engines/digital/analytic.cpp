#include <kiyosi/pricing/engines/digital/analytic.hpp>
#include "../../detail/common.hpp"
#include <cmath>
#include <ranges>

namespace kiyosi {
using namespace detail;

namespace {

PricingResult zero_tail(double value, std::optional<double> delta = std::nullopt,
                        std::optional<double> gamma = std::nullopt)
{
    PricingResult output{{risk_measure::price, value}};
    output.set(risk_measure::delta, delta);
    output.set(risk_measure::gamma, gamma);
    return output;
}

result<PricingResult> digital_price(double strike, option_type type, double payout,
                                    bool asset, date effective, date expiry, const PricingContext& context)
{
    const auto valid = validate_life(context.valuation_time(), effective, expiry);
    if (!valid) return std::unexpected(valid.error());
    const double spot = context.asset_price().value();
    const double t = actual_365(context.valuation_time(), expiry);
    const double sign = type == option_type::call ? 1.0 : -1.0;
    if (t == 0.0) {
        const bool exercised = sign * (spot - strike) > 0.0;
        auto output = zero_tail(exercised ? (asset ? spot : payout) : 0.0);
        return output;
    }
    const double sigma = context.parameters().volatility();
    const double root_t = std::sqrt(t);
    const double rate_df = std::exp(-context.parameters().risk_free_rate() * t);
    const double div_df = std::exp(-context.parameters().dividend_yield() * t);
    const double d1 = (std::log(spot / strike) +
                       (context.parameters().risk_free_rate() - context.parameters().dividend_yield() +
                        0.5 * sigma * sigma) *
                           t) /
                      (sigma * root_t);
    const double d2 = d1 - sigma * root_t;
    const double nd = normal_cdf(sign * (asset ? d1 : d2));
    const double density = normal_pdf(asset ? d1 : d2);
    const double scale = asset ? spot * div_df : payout * rate_df;
    const double value = scale * nd;
    double delta = 0.0;
    double gamma = 0.0;
    if (asset) {
        delta = div_df * (nd + sign * density / (sigma * root_t));
        gamma = -div_df * sign * density * d1 / (spot * sigma * sigma * t) +
                div_df * sign * density / (spot * sigma * root_t);
    } else {
        delta = payout * rate_df * sign * density / (spot * sigma * root_t);
        gamma = -payout * rate_df * sign * density *
                (1.0 + d2 / (sigma * root_t)) / (spot * spot * sigma * root_t);
    }
    auto output = zero_tail(value, delta, gamma);
    if (!output.all_finite())
        return std::unexpected(Error{error_category::invalid_result, "analytic pricing produced a non-finite result"});
    return output;
}

}

result<PricingResult> AnalyticDigitalEngine::price_impl(
    option_type type, double strike, double payout, bool asset, date effective, date expiry,
    const PricingContext& context) const
{
    return digital_price(strike, type, payout, asset, effective, expiry, context);
}

} // namespace kiyosi
