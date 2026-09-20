#include <kiyosi/pricing/engines/vanilla/integral.hpp>

#include <algorithm>
#include <cmath>

#include "../../detail/math.hpp"

namespace kiyosi {
using namespace detail;

Result<PricingResult> QuadratureVanillaEngine::price_impl(const EuropeanOption& option, const PricingContext& context) const
{
    auto valid = validate_valuation_within_instrument_life(context.valuation_time(), option.effective_date(), option.expiry_date());
    if (!valid) return std::unexpected(valid.error());
    const double tau = actual_365_fixed_year_fraction(context.valuation_time(), option.expiry_date());
    const double spot = context.spot_price();
    const double strike = option.strike();
    const double sign = option.option_type() == OptionType::call ? 1.0 : -1.0;
    if (tau == 0.0)
        return make_pricing_result(
            {{RiskMeasure::price, std::max(sign * (spot - strike), 0.0)}});
    const double sigma = context.model_parameters().volatility();
    const double rate = context.model_parameters().risk_free_rate();
    const double dividend = context.model_parameters().dividend_yield();
    const double root = std::sqrt(tau);
    if (sigma < 1e-12)
        return make_pricing_result(
            {{RiskMeasure::price,
              std::exp(-rate * tau) *
                  std::max(sign * (spot * std::exp((rate - dividend) * tau) - strike),
                           0.0)}});
    const double z_star = (std::log(strike / spot) - (rate - dividend - 0.5 * sigma * sigma) * tau) / (sigma * root);
    double lower = sign > 0 ? std::max(z_star, -10.0) : -10.0;
    double upper = sign > 0 ? 10.0 : std::min(z_star, 10.0);
    if (lower >= upper) return make_pricing_result({{RiskMeasure::price, 0.0}});
    constexpr int panels = 1024;
    const double h = (upper - lower) / panels;
    auto integrand = [&](double z) {
        const double terminal = spot * std::exp((rate - dividend - 0.5 * sigma * sigma) * tau + sigma * root * z);
        return std::max(sign * (terminal - strike), 0.0) * normal_pdf(z);
    };
    double sum = integrand(lower) + integrand(upper);
    for (int index = 1; index < panels; ++index)
        sum += (index % 2 ? 4.0 : 2.0) * integrand(lower + h * index);
    const double value = std::exp(-rate * tau) * sum * h / 3.0;
    if (!std::isfinite(value)) return std::unexpected(Error{ErrorCategory::invalid_result, "integral pricing produced a non-finite result"});
    return make_pricing_result({{RiskMeasure::price, value}});
}

} // namespace kiyosi
