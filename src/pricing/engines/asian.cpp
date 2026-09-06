#include <kiyosi/pricing/engines/asian.hpp>
#include "../detail/common.hpp"
#include <algorithm>
#include <cmath>

namespace kiyosi {
using namespace detail;
namespace {
double payoff(option_type type, double value, double strike)
{
    return std::max((type == option_type::call ? 1.0 : -1.0) * (value - strike), 0.0);
}
result<double> time_to_expiry(const PricingContext& context, date expiry)
{
    auto valid = validate_expiry(context.valuation_date(), expiry);
    if (!valid) return std::unexpected(valid.error());
    return actual_365(context.valuation_date(), expiry);
}
} // namespace

result<PricingResult> GeometricAverageAsianEngine::price(
    const GeometricAverageOption& option, const PricingContext& context) const
{
    auto tau_result = time_to_expiry(context, option.expiry());
    if (!tau_result) return std::unexpected(tau_result.error());
    const double tau = *tau_result;
    const double spot = context.asset_price().value();
    const double strike = option.strike();
    const double sign = option.type() == option_type::call ? 1.0 : -1.0;
    if (tau == 0.0) return PricingResult{{risk_measure::price, payoff(option.type(), option.realized_average() > 0.0 ? option.realized_average() : spot, strike)}};
    const double sigma = context.parameters().volatility();
    const double rate = context.parameters().risk_free_rate();
    const double carry = rate - context.parameters().dividend_yield();
    const double adjusted_sigma = sigma / std::sqrt(3.0);
    if (adjusted_sigma < 1e-12)
        return PricingResult{{risk_measure::price, std::exp(-rate * tau) * payoff(option.type(), spot * std::exp(carry * tau), strike)}};
    const double adjusted_carry = 0.5 * (carry - sigma * sigma / 6.0);
    const double root = std::sqrt(tau);
    const double d1 = (std::log(spot / strike) + (adjusted_carry + 0.5 * adjusted_sigma * adjusted_sigma) * tau) /
                      (adjusted_sigma * root);
    const double d2 = d1 - adjusted_sigma * root;
    const double value = sign * (spot * std::exp((adjusted_carry - rate) * tau) * normal_cdf(sign * d1) -
                                 strike * std::exp(-rate * tau) * normal_cdf(sign * d2));
    if (!std::isfinite(value)) return std::unexpected(Error{error_category::invalid_result, "Asian pricing produced a non-finite result"});
    return PricingResult{{risk_measure::price, std::max(value, 0.0)}};
}

result<PricingResult> ArithmeticAverageAsianEngine::price(
    const ArithmeticAverageOption& option, const PricingContext& context) const
{
    auto tau_result = time_to_expiry(context, option.expiry());
    if (!tau_result) return std::unexpected(tau_result.error());
    const double tau = *tau_result;
    const double spot = context.asset_price().value();
    const double strike = option.strike();
    const double sign = option.type() == option_type::call ? 1.0 : -1.0;
    const double rate = context.parameters().risk_free_rate();
    const double dividend = context.parameters().dividend_yield();
    const double carry = rate - dividend;
    const double sigma = context.parameters().volatility();
    if (tau == 0.0)
        return PricingResult{{risk_measure::price, payoff(option.type(), option.realized_average() > 0.0 ? option.realized_average() : spot, strike)}};
    const double average_period = actual_365(option.average_start(), option.expiry());
    if (average_period <= 0.0)
        return PricingResult{{risk_measure::price, payoff(option.type(), spot, strike)}};
    const double t1 = std::max(0.0, tau - average_period);
    const double remaining = average_period - tau;
    const double m1 = std::abs(carry) < 1e-12
                          ? 1.0
                          : (std::exp(carry * tau) - std::exp(carry * t1)) / (carry * (tau - t1));
    double adjusted_strike = strike;
    double scale = 1.0;
    if (remaining > 0.0) {
        adjusted_strike = average_period / tau * strike - remaining / tau * option.realized_average();
        scale = tau / average_period;
        if (adjusted_strike < 0.0) {
            if (sign < 0.0) return PricingResult{{risk_measure::price, 0.0}};
            const double expected = option.realized_average() * remaining / average_period + spot * m1 * tau / average_period;
            return PricingResult{{risk_measure::price, std::max(expected - strike, 0.0) * std::exp(-rate * tau)}};
        }
    }
    const double b_a = std::log(m1) / tau;
    const double vol2 = sigma * sigma;
    const double delta = tau - t1;
    const double delta2 = delta * delta;
    double m2;
    if (std::abs(carry) < 1e-12) {
        m2 = 2.0 * std::exp(vol2 * tau) / (vol2 * vol2 * delta2) -
             2.0 * std::exp(vol2 * t1) * (1.0 + vol2 * delta) / (vol2 * vol2 * delta2);
    } else {
        const double two = 2.0 * carry + vol2;
        const double one = carry + vol2;
        m2 = 2.0 * std::exp(two * tau) / (one * two * delta2) +
             2.0 * std::exp(two * t1) / (carry * delta2) * (1.0 / two - std::exp(carry * delta) / one);
    }
    const double adjusted_vol = std::sqrt(std::max(0.0, std::log(m2) / tau - 2.0 * b_a));
    const double root = adjusted_vol * std::sqrt(tau);
    if (root < 1e-12) {
        const double forward = spot * std::exp((rate - (rate - b_a)) * tau);
        return PricingResult{{risk_measure::price, scale * std::exp(-rate * tau) * payoff(option.type(), forward, adjusted_strike)}};
    }
    const double d1 = (std::log(spot / adjusted_strike) + (b_a + 0.5 * adjusted_vol * adjusted_vol) * tau) / root;
    const double d2 = d1 - root;
    const double value = scale * sign * (spot * std::exp((b_a - rate) * tau) * normal_cdf(sign * d1) - adjusted_strike * std::exp(-rate * tau) * normal_cdf(sign * d2));
    if (!std::isfinite(value)) return std::unexpected(Error{error_category::invalid_result, "Asian pricing produced a non-finite result"});
    return PricingResult{{risk_measure::price, std::max(value, 0.0)}};
}
} // namespace kiyosi
