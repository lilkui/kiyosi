#include <kiyosi/pricing/engines/asian/analytic.hpp>

#include <algorithm>
#include <cmath>

#include "../../detail/black_scholes.hpp"
#include "../../detail/math.hpp"

namespace kiyosi {
using namespace detail;
namespace {
double payoff(OptionType type, double value, double strike)
{
    return std::max((type == OptionType::call ? 1.0 : -1.0) * (value - strike), 0.0);
}
Result<double> time_to_expiry(const PricingContext& context, Date effective_date, Date expiry_date)
{
    auto valid = validate_valuation_within_instrument_life(context.valuation_time(), effective_date, expiry_date);
    if (!valid) return std::unexpected(valid.error());
    return actual_365_fixed_year_fraction(context.valuation_time(), expiry_date);
}
double exprel(double value)
{
    return value == 0.0 ? 1.0 : std::expm1(value) / value;
}
double divided_exprel(double x, double y)
{
    if (std::max(std::abs(x), std::abs(y)) < 0.5) {
        double sum = 0.5;
        double homogeneous = 1.0;
        double y_power = 1.0;
        double factorial = 2.0;
        for (int n = 1; n <= 16; ++n) {
            y_power *= y;
            homogeneous = x * homogeneous + y_power;
            factorial *= n + 2;
            sum += homogeneous / factorial;
        }
        return sum;
    }
    if (std::abs(x - y) < 1e-5) {
        const double midpoint = (x + y) / 2.0;
        return ((midpoint - 1.0) * std::exp(midpoint) + 1.0) / (midpoint * midpoint);
    }
    return (exprel(x) - exprel(y)) / (x - y);
}
} // namespace

Result<PricingResult> AnalyticGeometricAveragePriceEngine::price_native(
    const GeometricAveragePriceOption& option, const PricingContext& context) const
{
    auto tau_result = time_to_expiry(context, option.effective_date(), option.expiry_date());
    if (!tau_result) return std::unexpected(tau_result.error());
    const double tau = *tau_result;
    const double spot = context.spot_price();
    const double strike = option.strike();
    const double sign = option.option_type() == OptionType::call ? 1.0 : -1.0;
    const Timestamp valuation = context.valuation_time();
    const Timestamp averaging_start = start_of_day(option.averaging_start_date());
    const Timestamp expiry = start_of_day(option.expiry_date());
    const double realized = option.realized_average();
    if ((valuation > averaging_start && realized == 0.0) ||
        (valuation <= averaging_start && realized != 0.0))
        return std::unexpected(Error{ErrorCategory::invalid_parameter,
                                     "realized geometric average must match the elapsed averaging period"});
    if (tau == 0.0)
        return make_pricing_result({{RiskMeasure::price,
                                     payoff(option.option_type(), realized > 0.0 ? realized : spot, strike)}});
    const double sigma = context.model_parameters().volatility();
    const double rate = context.model_parameters().risk_free_rate();
    const double carry = rate - context.model_parameters().dividend_yield();
    const double period = actual_365_fixed_year_fraction(averaging_start, expiry);
    const double lead = valuation < averaging_start
                            ? actual_365_fixed_year_fraction(valuation, averaging_start)
                            : 0.0;
    const double future = actual_365_fixed_year_fraction(std::max(valuation, averaging_start), expiry);
    const double weight = period > 0.0 ? future / period : 1.0;
    // The future log-average has Brownian variance proportional to lead + future / 3.
    const double variance = sigma * sigma * weight * weight * (lead + future / 3.0);
    double mean_log = weight * std::log(spot) +
                      weight * (carry - 0.5 * sigma * sigma) * (lead + future / 2.0);
    if (valuation > averaging_start) mean_log += (1.0 - weight) * std::log(realized);
    const double forward = std::exp(mean_log + 0.5 * variance);
    const double deviation = std::sqrt(variance);
    double value;
    if (deviation < 1e-12) {
        value = std::exp(-rate * tau) * payoff(option.option_type(), forward, strike);
    } else {
        const double d1 = (mean_log - std::log(strike) + variance) / deviation;
        const double d2 = d1 - deviation;
        value = std::exp(-rate * tau) * sign *
                (forward * normal_cdf(sign * d1) - strike * normal_cdf(sign * d2));
    }
    if (!std::isfinite(value)) return std::unexpected(Error{ErrorCategory::invalid_result, "Asian pricing produced a non-finite result"});
    return make_pricing_result({{RiskMeasure::price, std::max(value, 0.0)}});
}

Result<PricingResult> TurnbullWakemanArithmeticAveragePriceEngine::price_native(
    const ArithmeticAveragePriceOption& option, const PricingContext& context) const
{
    auto tau_result = time_to_expiry(context, option.effective_date(), option.expiry_date());
    if (!tau_result) return std::unexpected(tau_result.error());
    const double tau = *tau_result;
    const Timestamp valuation = context.valuation_time();
    const Timestamp averaging_start = start_of_day(option.averaging_start_date());
    const double realized = option.realized_average();
    if ((valuation > averaging_start && realized == 0.0) ||
        (valuation <= averaging_start && realized != 0.0))
        return std::unexpected(Error{ErrorCategory::invalid_parameter,
                                     "realized arithmetic average must match the elapsed averaging period"});
    const double spot = context.spot_price();
    const double strike = option.strike();
    const double sign = option.option_type() == OptionType::call ? 1.0 : -1.0;
    const double rate = context.model_parameters().risk_free_rate();
    const double dividend = context.model_parameters().dividend_yield();
    const double carry = rate - dividend;
    const double sigma = context.model_parameters().volatility();
    if (tau == 0.0)
        return make_pricing_result(
            {{RiskMeasure::price,
              payoff(option.option_type(), realized > 0.0 ? realized : spot,
                     strike)}});
    if (option.averaging_start_date() == option.expiry_date()) {
        return price_at_volatility(
            *make_european_option(option.option_type(), strike, option.effective_date(), option.expiry_date()),
            context, sigma, RiskMeasureOutput::price_only);
    }
    const double average_period = actual_365_fixed_year_fraction(option.averaging_start_date(), option.expiry_date());
    const double t1 = std::max(0.0, tau - average_period);
    const double remaining = average_period - tau;
    const double delta = tau - t1;
    const double m1 = std::exp(carry * t1) * exprel(carry * delta);
    if (!std::isfinite(m1) || m1 <= 0.0)
        return std::unexpected(Error{ErrorCategory::invalid_result, "Asian pricing produced an invalid moment"});
    double adjusted_strike = strike;
    double scale = 1.0;
    if (remaining > 0.0) {
        adjusted_strike = average_period / tau * strike - remaining / tau * realized;
        scale = tau / average_period;
        if (adjusted_strike < 0.0) {
            if (sign < 0.0)
                return make_pricing_result({{RiskMeasure::price, 0.0}});
            const double expected = realized * remaining / average_period + spot * m1 * tau / average_period;
            return make_pricing_result(
                {{RiskMeasure::price,
                  std::max(expected - strike, 0.0) * std::exp(-rate * tau)}});
        }
    }
    const double vol2 = sigma * sigma;
    // The second moment is a divided difference of (exp(x) - 1) / x.
    const double m2 = 2.0 * std::exp((2.0 * carry + vol2) * t1) *
                      divided_exprel((2.0 * carry + vol2) * delta, carry * delta);
    if (!std::isfinite(m2) || m2 <= 0.0)
        return std::unexpected(Error{ErrorCategory::invalid_result, "Asian pricing produced an invalid moment"});
    const double log_variance = std::log(m2) - 2.0 * std::log(m1);
    if (!std::isfinite(log_variance) || log_variance < -1e-12)
        return std::unexpected(Error{ErrorCategory::invalid_result, "Asian pricing produced an invalid variance"});
    const double b_a = std::log(m1) / tau;
    const double adjusted_vol = std::sqrt(std::max(0.0, log_variance / tau));
    const double root = adjusted_vol * std::sqrt(tau);
    if (root < 1e-12) {
        const double forward = spot * std::exp((rate - (rate - b_a)) * tau);
        return make_pricing_result(
            {{RiskMeasure::price,
              scale * std::exp(-rate * tau) *
                  payoff(option.option_type(), forward, adjusted_strike)}});
    }
    const double d1 = (std::log(spot / adjusted_strike) + (b_a + 0.5 * adjusted_vol * adjusted_vol) * tau) / root;
    const double d2 = d1 - root;
    const double value = scale * sign * (spot * std::exp((b_a - rate) * tau) * normal_cdf(sign * d1) - adjusted_strike * std::exp(-rate * tau) * normal_cdf(sign * d2));
    if (!std::isfinite(value)) return std::unexpected(Error{ErrorCategory::invalid_result, "Asian pricing produced a non-finite result"});
    return make_pricing_result({{RiskMeasure::price, std::max(value, 0.0)}});
}
} // namespace kiyosi
