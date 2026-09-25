#include <kiyosi/pricing/engines/barrier/analytic.hpp>

#include <algorithm>
#include <array>
#include <cmath>
#include <limits>

#include "../../detail/black_scholes.hpp"
#include "../../detail/math.hpp"

namespace kiyosi {
using namespace detail;

namespace {

Result<PricingResult> make_price_delta_gamma_result(double value, std::optional<double> delta = std::nullopt,
                                                    std::optional<double> gamma = std::nullopt)
{
    return make_pricing_result({{RiskMeasure::price, value}, {RiskMeasure::delta, delta}, {RiskMeasure::gamma, gamma}});
}

double barrier_hit_discount(double distance, bool upper, double drift, double variance, double t, double rate)
{
    if (t == 0.0) return 1.0;
    const double signed_drift = upper ? -drift : drift;
    const double discriminant = signed_drift * signed_drift + 2.0 * rate * variance;
    const double scale = std::max({1.0, std::abs(signed_drift * signed_drift), std::abs(2.0 * rate * variance)});
    if (discriminant < 0.0 ||
        (rate < 0.0 && discriminant <= 16.0 * std::numeric_limits<double>::epsilon() * scale))
        return std::numeric_limits<double>::quiet_NaN();
    const double root = std::sqrt(discriminant);
    const double root_time = std::sqrt(variance * t);
    const double first = std::exp((-signed_drift - root) * distance / variance) *
                         normal_cdf((root * t - distance) / root_time);
    const double second = std::exp((-signed_drift + root) * distance / variance) *
                          normal_cdf((-root * t - distance) / root_time);
    const double result = first + second;
    return std::isfinite(result) ? result : std::numeric_limits<double>::quiet_NaN();
}

} // namespace

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
    const auto vanilla = price_at_volatility(
        *make_european_option(option.option_type(), option.strike(), option.effective_date(), option.expiry_date()), context,
        context.model_parameters().volatility(), RiskMeasureOutput::price_only);
    if (!vanilla) return std::unexpected(vanilla.error());
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
    if (option.observation_mode() == ObservationMode::scheduled) {
        barrier *= std::exp((upper ? 1.0 : -1.0) * bgk_beta * sigma *
                            std::sqrt(terms.mean_observation_year_fraction()));
    }
    if (touched) {
        const double touched_value = *vanilla->require(RiskMeasure::price);
        return make_price_delta_gamma_result(knock_in
                                                 ? touched_value
                                                 : option.rebate() * (option.rebate_timing() == RebateTiming::at_hit
                                                                          ? (*prior_touch ? 0.0 : 1.0)
                                                                          : std::exp(-rate * t)));
    }
    if (option.observation_mode() == ObservationMode::scheduled && !terms.has_remaining_observation(context.valuation_time()))
        return make_price_delta_gamma_result(knock_in ? option.rebate() * std::exp(-rate * t)
                                                       : *vanilla->require(RiskMeasure::price));
    if (option.rebate_timing() == RebateTiming::at_hit) {
        const double drift = rate - dividend - 0.5 * sigma * sigma;
        const double variance = sigma * sigma;
        const double hit_discount = barrier_hit_discount(std::abs(std::log(barrier / spot)), upper,
                                                         drift, variance, t, rate);
        if (!std::isfinite(hit_discount))
            return std::unexpected(Error{ErrorCategory::invalid_result,
                                         "barrier rebate discounting is numerically unstable"});
    }
    if (t == 0.0) {
        return knock_in ? make_price_delta_gamma_result(touched ? *vanilla->require(RiskMeasure::price) : option.rebate())
                        : make_price_delta_gamma_result(touched ? option.rebate() : *vanilla->require(RiskMeasure::price));
    }
    const double root_time = sigma * std::sqrt(t), discount = std::exp(-rate * t), carry = std::exp(-dividend * t);
    const double mu = (rate - dividend - 0.5 * sigma * sigma) / (sigma * sigma);
    const double lambda = std::sqrt(mu * mu + 2.0 * rate / (sigma * sigma));
    const double x = option.strike();
    const double x1 = std::log(spot / x) / root_time + (1.0 + mu) * root_time;
    const double x2 = std::log(spot / barrier) / root_time + (1.0 + mu) * root_time;
    const double y1 = std::log(barrier * barrier / (spot * x)) / root_time + (1.0 + mu) * root_time;
    const double y2 = std::log(barrier / spot) / root_time + (1.0 + mu) * root_time;
    const double z = std::log(barrier / spot) / root_time + lambda * root_time;
    const auto factors = [&](double eta, double phi) {
        const double ratio = barrier / spot;
        return std::array<double, 6>{
            phi * spot * carry * normal_cdf(phi * x1) - phi * x * discount * normal_cdf(phi * x1 - phi * root_time),
            phi * spot * carry * normal_cdf(phi * x2) - phi * x * discount * normal_cdf(phi * x2 - phi * root_time),
            phi * spot * carry * std::pow(ratio, 2.0 * (mu + 1.0)) * normal_cdf(eta * y1) -
                phi * x * discount * std::pow(ratio, 2.0 * mu) * normal_cdf(eta * y1 - eta * root_time),
            phi * spot * carry * std::pow(ratio, 2.0 * (mu + 1.0)) * normal_cdf(eta * y2) -
                phi * x * discount * std::pow(ratio, 2.0 * mu) * normal_cdf(eta * y2 - eta * root_time),
            option.rebate() * discount * (normal_cdf(eta * x2 - eta * root_time) - std::pow(ratio, 2.0 * mu) * normal_cdf(eta * y2 - eta * root_time)),
            option.rebate() * (option.rebate_timing() == RebateTiming::at_hit
                                   ? (std::pow(ratio, mu + lambda) * normal_cdf(eta * z) +
                                      std::pow(ratio, mu - lambda) * normal_cdf(eta * z - 2.0 * eta * lambda * root_time))
                                   : discount)};
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
    auto output = make_price_delta_gamma_result(value);
    return output;
}

} // namespace kiyosi
