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
        return make_pricing_result(
            {{RiskMeasure::price,
              sign * (spot - strike) > 0.0 ? (asset ? spot : payout) : 0.0}});
    const double rate = context.model_parameters().risk_free_rate();
    const double dividend = context.model_parameters().dividend_yield();
    const double volatility = context.model_parameters().volatility();
    const double root = std::sqrt(time);
    const double drift = (rate - dividend - 0.5 * volatility * volatility) * time;
    const double threshold = (std::log(strike / spot) - drift) / (volatility * root);
    const double lower = sign > 0.0 ? std::max(threshold, -12.0) : -12.0;
    const double upper = sign > 0.0 ? 12.0 : std::min(threshold, 12.0);
    if (lower >= upper) return make_pricing_result({{RiskMeasure::price, 0.0}});
    constexpr int panels = 2048;
    const double step = (upper - lower) / panels;
    auto integrand = [&](double z) {
        const double terminal = spot * std::exp(drift + volatility * root * z);
        // Bounds already select the ITM branch; use its one-sided limit at strike.
        return (asset ? terminal : payout) * normal_pdf(z);
    };
    double sum = integrand(lower) + integrand(upper);
    for (int index = 1; index < panels; ++index)
        sum += (index % 2 == 0 ? 2.0 : 4.0) * integrand(lower + index * step);
    const double value = std::exp(-rate * time) * sum * step / 3.0;
    if (!std::isfinite(value)) return std::unexpected(Error{ErrorCategory::invalid_result, "integral pricing produced a non-finite result"});
    return make_pricing_result({{RiskMeasure::price, value}});
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
