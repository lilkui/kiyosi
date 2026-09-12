#include <kiyosi/pricing/engines/digital/integral.hpp>
#include "../../detail/common.hpp"
#include <algorithm>
#include <cmath>

namespace kiyosi {
using namespace detail;

namespace {
result<PricingResult> price_digital_integral(option_type type, double strike, double payout, bool asset,
                                             date effective, date expiry, const PricingContext& context)
{
    const auto valid = validate_life(context.valuation_time(), effective, expiry);
    if (!valid) return std::unexpected(valid.error());
    const double time = actual_365(context.valuation_time(), expiry);
    const double spot = context.asset_price();
    const double sign = type == option_type::call ? 1.0 : -1.0;
    if (time == 0.0)
        return PricingResult{{risk_measure::price, sign * (spot - strike) > 0.0 ? (asset ? spot : payout) : 0.0}};
    const double rate = context.parameters().risk_free_rate();
    const double dividend = context.parameters().dividend_yield();
    const double volatility = context.parameters().volatility();
    const double root = std::sqrt(time);
    const double drift = (rate - dividend - 0.5 * volatility * volatility) * time;
    const double threshold = (std::log(strike / spot) - drift) / (volatility * root);
    const double lower = sign > 0.0 ? std::max(threshold, -12.0) : -12.0;
    const double upper = sign > 0.0 ? 12.0 : std::min(threshold, 12.0);
    if (lower >= upper) return PricingResult{{risk_measure::price, 0.0}};
    constexpr int panels = 2048;
    const double step = (upper - lower) / panels;
    auto integrand = [&](double z) {
        const double terminal = spot * std::exp(drift + volatility * root * z);
        // Bounds already select the ITM branch; use its one-sided limit at strike.
        return (asset ? terminal : payout) * normal_pdf(z);
    };
    double sum = integrand(lower) + integrand(upper);
    for (int index = 1; index < panels; ++index) sum += (index % 2 == 0 ? 2.0 : 4.0) * integrand(lower + index * step);
    const double value = std::exp(-rate * time) * sum * step / 3.0;
    if (!std::isfinite(value)) return std::unexpected(Error{error_category::invalid_result, "integral pricing produced a non-finite result"});
    return PricingResult{{risk_measure::price, value}};
}
}

result<PricingResult> IntegralDigitalEngine::price(const EuropeanCashOrNothingOption& option, const PricingContext& context) const
{
    return price_digital_integral(option.type(), option.strike(), option.payout(), false, option.effective(), option.expiry(), context);
}
result<PricingResult> IntegralDigitalEngine::price(const EuropeanAssetOrNothingOption& option, const PricingContext& context) const
{
    return price_digital_integral(option.type(), option.strike(), 1.0, true, option.effective(), option.expiry(), context);
}

} // namespace kiyosi
