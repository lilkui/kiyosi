#include <kiyosi/pricing/engines/digital.hpp>
#include <kiyosi/pricing/engines/barrier.hpp>
#include "../detail/common.hpp"
#include <algorithm>
#include <array>
#include <cmath>
#include <functional>
#include <limits>
#include <numbers>
#include <ranges>

namespace kiyosi {
using namespace detail;
namespace {

PricingResult zero_tail(double value, std::optional<double> delta = std::nullopt,
                        std::optional<double> gamma = std::nullopt)
{
    PricingResult output{{risk_measure::price, value}};
    output.set(risk_measure::delta, delta);
    output.set(risk_measure::gamma, gamma);
    return output;
}

result<PricingResult> digital_price(double strike, option_type type, double payout,
                                    bool asset, date expiry, const PricingContext& context)
{
    const auto valid = validate_expiry(context.valuation_date(), expiry);
    if (!valid) return std::unexpected(valid.error());
    const double spot = context.asset_price().value();
    const double t = actual_365(context.valuation_date(), expiry);
    const double sign = type == option_type::call ? 1.0 : -1.0;
    if (t == 0.0) {
        const bool exercised = sign * (spot - strike) > 0.0;
        auto output = zero_tail(exercised ? (asset ? spot : payout) : 0.0);
        return output;
    }
    const double sigma = context.parameters().volatility();
    const double root_t = std::sqrt(t);
    const double rate_df = std::exp(-context.parameters().risk_free_rate() * t);
    const double div_df = std::exp(-context.parameters().dividend_yield() * t);
    const double d1 = (std::log(spot / strike) +
                       (context.parameters().risk_free_rate() - context.parameters().dividend_yield() +
                        0.5 * sigma * sigma) *
                           t) /
                      (sigma * root_t);
    const double d2 = d1 - sigma * root_t;
    const double nd = normal_cdf(sign * (asset ? d1 : d2));
    const double density = normal_pdf(asset ? d1 : d2);
    const double scale = asset ? spot * div_df : payout * rate_df;
    const double value = scale * nd;
    double delta = 0.0;
    double gamma = 0.0;
    if (asset) {
        delta = div_df * (nd + sign * density / (sigma * root_t));
        gamma = -div_df * sign * density * d1 / (spot * sigma * sigma * t) +
                div_df * sign * density / (spot * sigma * root_t);
    } else {
        delta = payout * rate_df * sign * density / (spot * sigma * root_t);
        gamma = -payout * rate_df * sign * density *
                (1.0 + d2 / (sigma * root_t)) / (spot * spot * sigma * root_t);
    }
    auto output = zero_tail(value, delta, gamma);
    if (!std::ranges::all_of(output.values, [](const auto& item) {
            return !item || std::isfinite(*item);
        }))
        return std::unexpected(Error{error_category::invalid_result, "analytic pricing produced a non-finite result"});
    return output;
}

double simpson(const std::function<double(double)>& f, double a, double b, int n = 2048)
{
    // ponytail: fixed 2048-panel Simpson integration over +/-12 sigma; adaptive quadrature if parity needs tighter tails.
    if (b <= a) return 0.0;
    if (n % 2) ++n;
    const double h = (b - a) / n;
    double sum = f(a) + f(b);
    for (int i = 1; i < n; ++i)
        sum += (i % 2 ? 4.0 : 2.0) * f(a + i * h);
    return sum * h / 3.0;
}

double barrier_survival_density(double y, double boundary, bool upper, double drift, double variance, double t)
{
    const double x = upper ? boundary : -boundary;
    if ((upper && y >= boundary) || (!upper && y <= boundary)) return 0.0;
    const double mean = drift * t;
    const double sd = std::sqrt(variance * t);
    const auto normal = [sd](double z) {
        return std::exp(-0.5 * z * z) / (sd * std::sqrt(2.0 * std::numbers::pi));
    };
    const double reflected = std::exp(-2.0 * (upper ? -drift : drift) * x / variance);
    if (upper) return normal(y - mean) - reflected * normal(y - 2.0 * boundary - mean);
    return normal(y - mean) - reflected * normal(y - 2.0 * boundary - mean);
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

result<PricingResult> AnalyticDigitalEngine::price(
    const CashOrNothingOption& option, const PricingContext& context) const
{
    return select_outputs(digital_price(option.strike(), option.type(), option.payout(), false, option.expiry(), context),
                          PricingRequest{supported_risk_measures}, supported_risk_measures);
}

result<PricingResult> AnalyticDigitalEngine::price(
    const CashOrNothingOption& option, const PricingContext& context, PricingRequest request) const
{
    return select_outputs(price(option, context), request, supported_risk_measures);
}

result<PricingResult> AnalyticDigitalEngine::price(
    const AssetOrNothingOption& option, const PricingContext& context) const
{
    return select_outputs(digital_price(option.strike(), option.type(), 1.0, true, option.expiry(), context),
                          PricingRequest{supported_risk_measures}, supported_risk_measures);
}

result<PricingResult> AnalyticDigitalEngine::price(
    const AssetOrNothingOption& option, const PricingContext& context, PricingRequest request) const
{
    return select_outputs(price(option, context), request, supported_risk_measures);
}

result<PricingResult> AnalyticBarrierEngine::price(
    const BarrierOption& option, const PricingContext& context) const
{
    const auto valid = validate_expiry(context.valuation_date(), option.expiry());
    if (!valid) return std::unexpected(valid.error());
    if (option.observation() == observation_mode::scheduled) {
        auto schedule_valid = validate_schedule(option.schedule(), context.valuation_date(),
                                                option.expiry(), context.calendar());
        if (!schedule_valid)
            return std::unexpected(Error{error_category::invalid_schedule, schedule_valid.error().message});
        // ponytail: scheduled dates use a BGK barrier shift; exact discrete monitoring needs a separate engine.
    }
    const auto vanilla = price_at_volatility(
        *make_european_option(option.type(), option.strike(), option.expiry()), context,
        context.parameters().volatility(), PricingRequest::price_only());
    if (!vanilla) return std::unexpected(vanilla.error());
    const double t = actual_365(context.valuation_date(), option.expiry());
    const double spot = context.asset_price().value();
    const double rate = context.parameters().risk_free_rate();
    const double dividend = context.parameters().dividend_yield();
    const double sigma = context.parameters().volatility();
    double barrier = option.barrier();
    const bool upper = option.barrier_kind() == barrier_type::up_and_in ||
                       option.barrier_kind() == barrier_type::up_and_out;
    const bool knock_in = option.barrier_kind() == barrier_type::up_and_in ||
                          option.barrier_kind() == barrier_type::down_and_in;
    if (option.observation() == observation_mode::scheduled) {
        const double interval = t / static_cast<double>(option.observation_dates().size());
        barrier *= std::exp((upper ? 1.0 : -1.0) * 0.5825971579 * sigma * std::sqrt(interval));
    }
    const bool touched = upper ? spot >= barrier : spot <= barrier;
    if (t == 0.0) {
        return knock_in ? zero_tail(touched ? *vanilla->get(risk_measure::price) : option.rebate())
                        : zero_tail(touched ? option.rebate() : *vanilla->get(risk_measure::price));
    }
    const double drift = rate - dividend - 0.5 * sigma * sigma;
    const double variance = sigma * sigma;
    const double boundary = std::log(barrier / spot);
    double survival_value = 0.0;
    double survival_probability = 0.0;
    if (!touched) {
        const double sd = sigma * std::sqrt(t);
        const double mean = drift * t;
        const double lo = mean - 12.0 * sd;
        const double hi = mean + 12.0 * sd;
        const double lower = upper ? lo : std::max(lo, boundary);
        const double higher = upper ? std::min(hi, boundary) : hi;
        const double sign = option.type() == option_type::call ? 1.0 : -1.0;
        const auto density = [&](double y) { return barrier_survival_density(y, boundary, upper, drift, variance, t); };
        survival_probability = simpson(density, lower, higher);
        survival_value = simpson([&](double y) {
            return std::max(sign * (spot * std::exp(y) - option.strike()), 0.0) * density(y) * std::exp(-rate * t);
        },
                                 lower, higher);
    }
    double value = knock_in ? *vanilla->get(risk_measure::price) - survival_value : survival_value;
    if (knock_in && option.rebate() > 0.0 && !touched) {
        value += option.rebate() * std::exp(-rate * t) * survival_probability;
    }
    if (!knock_in && option.rebate() > 0.0) {
        double rebate_factor = std::exp(-rate * t) * (1.0 - survival_probability);
        if (option.rebate_payment() == rebate_timing::at_hit && !touched) {
            const double distance = std::abs(boundary);
            rebate_factor = barrier_hit_discount(distance, upper, drift, variance, t, rate);
            if (!std::isfinite(rebate_factor))
                return std::unexpected(Error{error_category::invalid_result,
                                             "barrier rebate discounting is numerically unstable"});
        }
        value += option.rebate() * (touched ? (option.rebate_payment() == rebate_timing::at_hit ? 1.0 : std::exp(-rate * t)) : rebate_factor);
    }
    if (!std::isfinite(value))
        return std::unexpected(Error{error_category::invalid_result, "analytic pricing produced a non-finite result"});
    auto output = zero_tail(value);
    return output;
}

result<PricingResult> AnalyticBarrierEngine::price(
    const BarrierOption& option, const PricingContext& context, PricingRequest request) const
{
    return select_outputs(price(option, context), request, supported_risk_measures);
}

} // namespace kiyosi
