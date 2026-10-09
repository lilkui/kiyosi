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
double divided_exprel_increment(double x, double y, double h)
{
    // Factor h out of the second divided difference instead of subtracting near-equal moments.
    if (std::max(std::abs(x + h), std::abs(y)) < 0.5) {
        double sum = 1.0 / 6.0;
        double homogeneous_two = 1.0;
        double homogeneous_three = 1.0;
        double y_power = 1.0;
        double factorial = 6.0;
        for (int n = 1; n <= 16; ++n) {
            y_power *= y;
            homogeneous_two = x * homogeneous_two + y_power;
            homogeneous_three = (x + h) * homogeneous_three + homogeneous_two;
            factorial *= n + 3;
            sum += homogeneous_three / factorial;
        }
        return h * sum;
    }
    const double first_increment = (std::exp(x) * exprel(h) - exprel(x)) / (x + h);
    return h * (first_increment - divided_exprel(x, y)) / (x + h - y);
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
        return make_pricing_result(payoff(option.option_type(), realized > 0.0 ? realized : spot, strike));
    const double sigma = context.model_parameters().volatility();
    if (option.averaging_start_date() == option.expiry_date()) {
        return price_at_volatility(
            *make_european_option(option.option_type(), strike, option.effective_date(), option.expiry_date()),
            context, sigma, GreeksRequest{});
    }
    const double rate = context.model_parameters().risk_free_rate();
    const double carry = rate - context.model_parameters().dividend_yield();
    const double period = actual_365_fixed_year_fraction(averaging_start, expiry);
    const double lead = valuation < averaging_start
                            ? actual_365_fixed_year_fraction(valuation, averaging_start) // NOLINT(readability-suspicious-call-argument): valuation precedes averaging.
                            : 0.0;
    const double future = actual_365_fixed_year_fraction(std::max(valuation, averaging_start), expiry);
    const double weight = period > 0.0 ? future / period : 1.0;
    // The future log-average has Brownian variance proportional to lead + future / 3.
    const double variance_time = lead + future / 3.0;
    const double variance = sigma * sigma * weight * weight * variance_time;
    double log_forward_ratio = weight * (carry - 0.5 * sigma * sigma) * (lead + future / 2.0) + 0.5 * variance;
    if (valuation > averaging_start) {
        const double relative_realized = (realized - spot) / spot;
        const double log_realized_ratio = std::abs(relative_realized) < 0.5
                                              ? std::log1p(relative_realized)
                                              : std::log(realized) - std::log(spot);
        log_forward_ratio += (1.0 - weight) * log_realized_ratio;
    }
    const double deviation = sigma * weight * std::sqrt(variance_time);
    if (deviation < 1e-5) {
        const auto parameters = make_bsm_parameters(rate, rate - log_forward_ratio / tau, sigma);
        if (!parameters)
            return std::unexpected(Error{ErrorCategory::invalid_result, "Asian pricing produced an invalid carry"});
        const auto equivalent_context = *make_pricing_context(*parameters, spot, valuation, context.calendar());
        const auto equivalent_option = *make_european_option(option.option_type(), strike, option.effective_date(), option.expiry_date());
        return price_at_volatility(equivalent_option, equivalent_context,
                                   sigma * weight * std::sqrt(variance_time / tau), GreeksRequest{});
    }
    const double forward = spot * std::exp(log_forward_ratio);
    const double value = [&] {
        const double relative_spot = (spot - strike) / strike;
        const double log_moneyness = std::abs(relative_spot) < 0.5
                                        ? std::log1p(relative_spot)
                                        : std::log(spot) - std::log(strike);
        const double d1 = (log_moneyness + log_forward_ratio + 0.5 * variance) / deviation;
        const double d2 = d1 - deviation;
        return std::exp(-rate * tau) * sign *
               (forward * normal_cdf(sign * d1) - strike * normal_cdf(sign * d2));
    }();
    if (!std::isfinite(value)) return std::unexpected(Error{ErrorCategory::invalid_result, "Asian pricing produced a non-finite result"});
    return make_pricing_result(std::max(value, 0.0));
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
        return make_pricing_result(payoff(option.option_type(), realized > 0.0 ? realized : spot,
                                          strike));
    if (option.averaging_start_date() == option.expiry_date()) {
        return price_at_volatility(
            *make_european_option(option.option_type(), strike, option.effective_date(), option.expiry_date()),
            context, sigma, GreeksRequest{});
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
        adjusted_strike = strike + remaining / tau * (strike - realized);
        scale = tau / average_period;
        if (adjusted_strike < 0.0) {
            if (sign < 0.0)
                return make_pricing_result(0.0);
            const double expected = realized * remaining / average_period + spot * m1 * tau / average_period;
            return make_pricing_result(std::max(expected - strike, 0.0) * std::exp(-rate * tau));
        }
    }
    const double vol2 = sigma * sigma;
    const double log_variance = [&] {
        if (vol2 * tau < 1e-4) {
            const double mean = exprel(carry * delta);
            const double excess = 2.0 * divided_exprel_increment(2.0 * carry * delta, carry * delta, vol2 * delta) / (mean * mean);
            return vol2 * t1 + std::log1p(excess);
        }
        // The second moment is a divided difference of (exp(x) - 1) / x.
        const double m2 = 2.0 * std::exp((2.0 * carry + vol2) * t1) *
                          divided_exprel((2.0 * carry + vol2) * delta, carry * delta);
        return std::log(m2) - 2.0 * std::log(m1);
    }();
    if (!std::isfinite(log_variance) || log_variance < -1e-12)
        return std::unexpected(Error{ErrorCategory::invalid_result, "Asian pricing produced an invalid variance"});
    const double b_a = std::log(m1) / tau;
    const double adjusted_vol = std::sqrt(std::max(0.0, log_variance / tau));
    const double root = adjusted_vol * std::sqrt(tau);
    if (root == 0.0) {
        const double forward = spot * std::exp((rate - (rate - b_a)) * tau);
        return make_pricing_result(scale * std::exp(-rate * tau) *
                                   payoff(option.option_type(), forward, adjusted_strike));
    }
    if (root < 1e-5 && adjusted_strike > 0.0) {
        const auto parameters = make_bsm_parameters(rate, rate - b_a, sigma);
        if (!parameters)
            return std::unexpected(Error{ErrorCategory::invalid_result, "Asian pricing produced an invalid carry"});
        const auto equivalent_context = *make_pricing_context(*parameters, spot, valuation, context.calendar());
        const auto equivalent_option = make_european_option(option.option_type(), adjusted_strike, option.effective_date(), option.expiry_date());
        if (!equivalent_option)
            return std::unexpected(Error{ErrorCategory::invalid_result, "Asian pricing produced an invalid strike"});
        const auto priced = price_at_volatility(*equivalent_option, equivalent_context, adjusted_vol, GreeksRequest{});
        if (!priced) return std::unexpected(priced.error());
        return make_pricing_result(scale * priced->price());
    }
    const double d1 = (std::log(spot / adjusted_strike) + (b_a + 0.5 * adjusted_vol * adjusted_vol) * tau) / root;
    const double d2 = d1 - root;
    const double value = scale * sign * (spot * std::exp((b_a - rate) * tau) * normal_cdf(sign * d1) - adjusted_strike * std::exp(-rate * tau) * normal_cdf(sign * d2));
    if (!std::isfinite(value)) return std::unexpected(Error{ErrorCategory::invalid_result, "Asian pricing produced a non-finite result"});
    return make_pricing_result(std::max(value, 0.0));
}
} // namespace kiyosi
