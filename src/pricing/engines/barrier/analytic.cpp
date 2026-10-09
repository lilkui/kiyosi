#include <kiyosi/pricing/engines/barrier/analytic.hpp>

#include <algorithm>
#include <array>
#include <cmath>
#include <limits>

#include "../../detail/black_scholes.hpp"
#include "../../detail/math.hpp"

namespace kiyosi {
using namespace detail;

Result<PricingResult> AnalyticBarrierEngine::price_native(
    const BarrierOption& option, const PricingContext& context) const
{
    const auto valid = validate_valuation_within_instrument_life(context.valuation_time(), option.effective_date(), option.expiry_date());
    if (!valid) return std::unexpected(valid.error());
    if (option.observation_mode() == ObservationMode::scheduled) {
        auto schedule_valid = validate_observation_dates(option.observation_dates(), option.effective_date(),
                                                         option.expiry_date(), context.calendar());
        if (!schedule_valid) return std::unexpected(schedule_valid.error());
        // ponytail: scheduled dates use a BGK barrier shift; exact discrete monitoring needs a separate engine.
    }
    const double t = actual_365_fixed_year_fraction(context.valuation_time(), option.expiry_date());
    const double spot = context.spot_price();
    const double rate = context.model_parameters().risk_free_rate();
    const double dividend = context.model_parameters().dividend_yield();
    const double sigma = context.model_parameters().volatility();
    const auto& terms = option.barrier_terms();
    const auto prior_touch = terms.was_touched_before(context.valuation_time());
    if (!prior_touch) return std::unexpected(prior_touch.error());
    double barrier = terms.barrier_level();
    const bool upper = terms.is_up();
    const bool knock_in = terms.is_knock_in();
    const bool touched_now = terms.is_monitored_at(context.valuation_time()) && terms.is_breached_by(spot);
    const bool touched = *prior_touch || touched_now;
    if (touched && !knock_in)
        return make_pricing_result(option.rebate() * (option.rebate_timing() == RebateTiming::at_hit
                                                          ? (*prior_touch ? 0.0 : 1.0)
                                                          : std::exp(-rate * t)));
    const bool monitoring_finished = !terms.is_continuous() &&
                                     start_of_day(terms.observation_dates().back()) <= context.valuation_time();
    if (!touched && monitoring_finished && knock_in)
        return make_pricing_result(option.rebate() * std::exp(-rate * t));
    const auto vanilla = price_at_volatility(
        *make_european_option(option.option_type(), option.strike(), option.effective_date(), option.expiry_date()), context,
        context.model_parameters().volatility(), GreeksRequest{});
    if (!vanilla) return std::unexpected(vanilla.error());
    if (touched || monitoring_finished)
        return make_pricing_result(vanilla->price());
    const auto monitoring_valid = validate_analytic_barrier_monitoring_window(terms);
    if (!monitoring_valid) return std::unexpected(monitoring_valid.error());
    if (option.observation_mode() == ObservationMode::scheduled) {
        barrier *= std::exp((upper ? 1.0 : -1.0) * bgk_beta * sigma *
                            std::sqrt(terms.mean_observation_year_fraction()));
    }
    if (t == 0.0)
        return make_pricing_result(knock_in ? option.rebate() : vanilla->price());
    const bool hit_rebate = option.rebate() != 0.0 && option.rebate_timing() == RebateTiming::at_hit;
    const double log_ratio = log_price_ratio(barrier, spot);
    double hit_discount = 0.0;
    if (hit_rebate) {
        const double drift = rate - dividend - 0.5 * sigma * sigma;
        const double variance = sigma * sigma;
        const double discriminant = drift * drift + 2.0 * rate * variance;
        const double scale = std::max({1.0, std::abs(drift * drift), std::abs(2.0 * rate * variance)});
        if (rate < 0.0 && discriminant <= 16.0 * std::numeric_limits<double>::epsilon() * scale)
            return std::unexpected(Error{ErrorCategory::invalid_result,
                                         "barrier rebate discounting is numerically unstable"});
        hit_discount = barrier_hit_discount(std::abs(log_ratio), upper,
                                            drift, variance, t, rate);
        if (!std::isfinite(hit_discount))
            return std::unexpected(Error{ErrorCategory::invalid_result,
                                         "barrier rebate discounting is numerically unstable"});
    }
    const double root_time = sigma * std::sqrt(t), discount = std::exp(-rate * t), carry = std::exp(-dividend * t);
    const double mu = (rate - dividend - 0.5 * sigma * sigma) / (sigma * sigma);
    const double x = option.strike();
    const double log_moneyness = log_price_ratio(spot, x);
    const double x1 = log_moneyness / root_time + (1.0 + mu) * root_time;
    const double x2 = -log_ratio / root_time + (1.0 + mu) * root_time;
    const double y1 = (2.0 * log_ratio + log_moneyness) / root_time + (1.0 + mu) * root_time;
    const double y2 = log_ratio / root_time + (1.0 + mu) * root_time;
    const auto factors = [&](double eta, double phi) {
        return std::array<double, 6>{
            phi * spot * carry * normal_cdf(phi * x1) - phi * x * discount * normal_cdf(phi * x1 - phi * root_time),
            phi * spot * carry * normal_cdf(phi * x2) - phi * x * discount * normal_cdf(phi * x2 - phi * root_time),
            phi * spot * carry * exponential_normal_cdf((2.0 * (mu + 1.0)) * log_ratio, eta * y1) -
                phi * x * discount * exponential_normal_cdf((2.0 * mu) * log_ratio, eta * y1 - eta * root_time),
            phi * spot * carry * exponential_normal_cdf((2.0 * (mu + 1.0)) * log_ratio, eta * y2) -
                phi * x * discount * exponential_normal_cdf((2.0 * mu) * log_ratio, eta * y2 - eta * root_time),
            option.rebate() * discount * (normal_cdf(eta * x2 - eta * root_time) - exponential_normal_cdf((2.0 * mu) * log_ratio, eta * y2 - eta * root_time)),
            option.rebate() * (hit_rebate ? hit_discount : discount)};
    };
    const bool call = option.option_type() == OptionType::call;
    const double eta = upper ? -1.0 : 1.0;
    const auto f = factors(eta, call ? 1.0 : -1.0);
    const auto rebate = [&](const std::array<double, 6>& values) { return option.rebate_timing() == RebateTiming::at_hit ? values[5] : option.rebate() * discount - values[4]; };
    double value = 0.0;
    if (call) {
        if (knock_in) value = upper ? (x > barrier ? f[0] + f[4] : f[1] - f[2] + f[3] + f[4]) : (x > barrier ? f[2] + f[4] : f[0] - f[1] + f[3] + f[4]);
        else value = upper ? (x > barrier ? rebate(f) : f[0] - f[1] + f[2] - f[3] + rebate(f)) : (x > barrier ? f[0] - f[2] + rebate(f) : f[1] - f[3] + rebate(f));
    } else {
        if (knock_in) value = upper ? (x > barrier ? f[0] - f[1] + f[3] + f[4] : f[2] + f[4]) : (x > barrier ? f[1] - f[2] + f[3] + f[4] : f[0] + f[4]);
        else value = upper ? (x > barrier ? f[1] - f[3] + rebate(f) : f[0] - f[2] + rebate(f)) : (x > barrier ? f[0] - f[1] + f[2] - f[3] + rebate(f) : rebate(f));
    }
    if (!std::isfinite(value))
        return std::unexpected(Error{ErrorCategory::invalid_result, "analytic pricing produced a non-finite result"});
    return make_pricing_result(value);
}

} // namespace kiyosi
