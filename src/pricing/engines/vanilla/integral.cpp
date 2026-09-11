#include <kiyosi/pricing/engines/vanilla/integral.hpp>
#include "../../detail/common.hpp"
#include <algorithm>
#include <cmath>

namespace kiyosi {
using namespace detail;

result<PricingResult> IntegralEuropeanEngine::price(const EuropeanOption& option, const PricingContext& context) const
{
    auto valid = validate_life(context.valuation_time(), option.effective(), option.expiry());
    if (!valid) return std::unexpected(valid.error());
    const double tau = actual_365(context.valuation_time(), option.expiry());
    const double spot = context.asset_price().value();
    const double strike = option.strike();
    const double sign = option.type() == option_type::call ? 1.0 : -1.0;
    if (tau == 0.0) return PricingResult{{risk_measure::price, std::max(sign * (spot - strike), 0.0)}};
    const double sigma = context.parameters().volatility();
    const double rate = context.parameters().risk_free_rate();
    const double dividend = context.parameters().dividend_yield();
    const double root = std::sqrt(tau);
    if (sigma < 1e-12)
        return PricingResult{{risk_measure::price, std::exp(-rate * tau) * std::max(sign * (spot * std::exp((rate - dividend) * tau) - strike), 0.0)}};
    const double z_star = (std::log(strike / spot) - (rate - dividend - 0.5 * sigma * sigma) * tau) / (sigma * root);
    double lower = sign > 0 ? std::max(z_star, -10.0) : -10.0;
    double upper = sign > 0 ? 10.0 : std::min(z_star, 10.0);
    if (lower >= upper) return PricingResult{{risk_measure::price, 0.0}};
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
    if (!std::isfinite(value)) return std::unexpected(Error{error_category::invalid_result, "integral pricing produced a non-finite result"});
    return PricingResult{{risk_measure::price, value}};
}

} // namespace kiyosi
