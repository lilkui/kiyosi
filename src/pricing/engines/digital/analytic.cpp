#include <kiyosi/pricing/engines/digital/analytic.hpp>

#include <cmath>
#include <optional>

#include "../../detail/math.hpp"

namespace kiyosi {
using namespace detail;

namespace {

Result<PricingResult> make_price_delta_gamma_result(double value, std::optional<double> delta = std::nullopt,
                                std::optional<double> gamma = std::nullopt)
{
    return make_pricing_result({{RiskMeasure::price, value}, {RiskMeasure::delta, delta},
                                {RiskMeasure::gamma, gamma}});
}

Result<PricingResult> digital_price(double strike, OptionType type, double payout,
                                    bool asset_settlement, Date effective_date, Date expiry_date, const PricingContext& context)
{
    const auto valid = validate_valuation_within_instrument_life(context.valuation_time(), effective_date, expiry_date);
    if (!valid) return std::unexpected(valid.error());
    const double spot = context.spot_price();
    const double t = actual_365_fixed_year_fraction(context.valuation_time(), expiry_date);
    const double sign = type == OptionType::call ? 1.0 : -1.0;
    if (t == 0.0) {
        const bool exercised = sign * (spot - strike) > 0.0;
        auto output = make_price_delta_gamma_result(exercised ? (asset_settlement ? spot : payout) : 0.0);
        return output;
    }
    const double sigma = context.model_parameters().volatility();
    const double root_t = std::sqrt(t);
    const double rate_df = std::exp(-context.model_parameters().risk_free_rate() * t);
    const double div_df = std::exp(-context.model_parameters().dividend_yield() * t);
    const double d1 = (std::log(spot / strike) +
                       (context.model_parameters().risk_free_rate() - context.model_parameters().dividend_yield() +
                        0.5 * sigma * sigma) *
                           t) /
                      (sigma * root_t);
    const double d2 = d1 - sigma * root_t;
    const double nd = normal_cdf(sign * (asset_settlement ? d1 : d2));
    const double density = normal_pdf(asset_settlement ? d1 : d2);
    const double scale = asset_settlement ? spot * div_df : payout * rate_df;
    const double value = scale * nd;
    double delta = 0.0;
    double gamma = 0.0;
    if (asset_settlement) {
        delta = div_df * (nd + sign * density / (sigma * root_t));
        gamma = -div_df * sign * density * d1 / (spot * sigma * sigma * t) +
                div_df * sign * density / (spot * sigma * root_t);
    } else {
        delta = payout * rate_df * sign * density / (spot * sigma * root_t);
        gamma = -payout * rate_df * sign * density *
                (1.0 + d2 / (sigma * root_t)) / (spot * spot * sigma * root_t);
    }
    auto output = make_price_delta_gamma_result(value, delta, gamma);
    if (!output) return std::unexpected(output.error());
    if (!output->all_finite())
        return std::unexpected(Error{ErrorCategory::invalid_result, "analytic pricing produced a non-finite result"});
    return output;
}

}

Result<PricingResult> AnalyticDigitalEngine::price_impl(
    OptionType type, double strike, double payout, bool asset_settlement, Date effective_date, Date expiry_date,
    const PricingContext& context) const
{
    return digital_price(strike, type, payout, asset_settlement, effective_date, expiry_date, context);
}

} // namespace kiyosi
