#include <kiyosi/pricing/engines/barrier/double_analytic.hpp>

#include <algorithm>
#include <array>
#include <cmath>
#include <numbers>
#include <vector>

#include "../../detail/barrier_settlement.hpp"
#include "../../detail/double_barrier.hpp"
#include "../../detail/numerical_greeks.hpp"
#include "../../detail/quadrature.hpp"

namespace kiyosi {
namespace {
using namespace detail;

struct AbsorbingDistribution {
    double initial;
    double band;
    double log_lower;
    double volatility;
    double drift;
    double time;

    // Integrate exp(order * log(asset)) against the killed transition density.
    // The complement is used only for total exit probability, avoiding 1-survival cancellation.
    Result<double> moment(int order, double lower, double upper, double discount = 0.0, bool complement = false) const
    {
        if (lower >= upper) return 0.0;
        const double deviation = volatility * std::sqrt(time);
        const double variance = deviation * deviation;
        const double ratio = deviation / band;
        if (!(deviation > 0.0) || !std::isfinite(deviation) || !std::isfinite(drift * time))
            return std::unexpected(Error{ErrorCategory::invalid_result, "double-barrier transition is not representable"});
        quadrature::Sum sum;
        double scale = 0.0;
        bool converged = false;
        if (ratio < 0.5) {
            const double base = order * (log_lower + initial + drift * time) + 0.5 * order * order * variance + discount;
            const double mean = initial + drift * time + order * variance;
            const auto image = [&](double k, bool reflected) {
                const double distance = reflected ? initial + k * band : k * band;
                const double center = mean - 2.0 * distance;
                const double exponent = base - 2.0 * distance * ((drift / volatility) / volatility + order);
                const double low = (lower - center) / deviation, high = (upper - center) / deviation;
                if (low >= high) return 0.0;
                if (low <= 10.0 && high >= -10.0)
                    return std::exp(exponent + quadrature::log_interval_mass(low, high));
                // Combine the reflection and Gaussian exponents before low-volatility cancellation.
                const auto tail = [&](double boundary, double z) {
                    const double free_z = (boundary - mean) / deviation;
                    const double gaussian = base - 0.5 * free_z * free_z -
                                            2.0 * (distance / deviation) * ((boundary - initial + distance) / deviation);
                    const double inverse_square = (1.0 / z) / z;
                    double term = 1.0, series = 1.0;
                    for (int index = 1; index <= 20; ++index) {
                        term *= -(2.0 * index - 1.0) * inverse_square;
                        series += term;
                    }
                    return gaussian - std::log(-z) + std::log(inverse_sqrt_two_pi * series);
                };
                const double large = high < 0.0 ? tail(upper, high) : tail(lower, -low);
                const double small = high < 0.0 ? tail(lower, low) : tail(upper, -high);
                return std::exp(large + std::log(-std::expm1(small - large)));
            };
            if (complement) {
                sum.add(scaled_normal_cdf(1.0, discount, -mean / deviation));
                sum.add(scaled_normal_cdf(1.0, discount, (mean - band) / deviation));
            } else {
                sum.add(image(0.0, false));
            }
            const double orientation = complement ? -1.0 : 1.0;
            sum.add(-orientation * image(0.0, true));
            scale = std::max(1.0, std::abs(sum.total()));
            for (int index = 1; index <= 64; ++index) {
                const double k = index;
                const std::array terms{image(k, false), image(k, true), image(-k, false), image(-k, true)};
                sum.add(orientation * terms[0]);
                sum.add(-orientation * terms[1]);
                sum.add(orientation * terms[2]);
                sum.add(-orientation * terms[3]);
                const double tail = terms[0] + terms[1] + terms[2] + terms[3];
                if (index >= 2 && tail <= 1e-16 * scale) {
                    converged = true;
                    break;
                }
            }
        } else {
            const double drift_ratio = (drift / volatility) / volatility;
            const double slope = (drift_ratio + order) * band;
            const auto exponent = [&](double y) {
                return discount + order * (log_lower + y) + drift_ratio * (y - initial) -
                       0.5 * (drift / volatility) * (drift / volatility) * time;
            };
            const double first_exponent = exponent(lower), last_exponent = exponent(upper);
            for (int index = 1; index <= 256; ++index) {
                const double frequency = index * std::numbers::pi;
                const double decay = 0.5 * (frequency * ratio) * (frequency * ratio);
                const double denominator = std::hypot(slope, frequency);
                const auto endpoint = [&](double y, double log_weight) {
                    const double sine = y == 0.0 || y == band ? 0.0 : std::sin(frequency * (y / band));
                    const double cosine = y == 0.0 ? 1.0 : y == band ? (index % 2 == 0 ? 1.0 : -1.0)
                                                                     : std::cos(frequency * (y / band));
                    const double coefficient = ((slope / denominator) * sine - (frequency / denominator) * cosine) / denominator;
                    return scaled_exponential(coefficient, log_weight - decay);
                };
                const double term = 2.0 * std::sin(frequency * (initial / band)) *
                                    (endpoint(upper, last_exponent) - endpoint(lower, first_exponent));
                sum.add(term);
                scale = std::max({1.0, scale, std::abs(term)});
                const double next_frequency = (index + 1.0) * std::numbers::pi;
                const double tail_exponent = std::max(first_exponent, last_exponent) -
                                             0.5 * (next_frequency * ratio) * (next_frequency * ratio);
                if (index >= 4 && tail_exponent < std::log(scale) - 40.0) {
                    converged = true;
                    break;
                }
            }
            if (complement) {
                quadrature::Sum difference;
                difference.add(std::exp(discount));
                difference.add(-sum.total());
                sum = difference;
            }
        }
        const double value = sum.total();
        if (!converged || !std::isfinite(value) || value < -1e-12 * scale)
            return std::unexpected(Error{ErrorCategory::invalid_result, "double-barrier density series did not converge to a representable moment"});
        return std::max(0.0, value);
    }
};

AbsorbingDistribution distribution(const DoubleBarrierOption& option, const PricingContext& context, double time)
{
    const auto& model = context.model_parameters();
    const double sigma = model.volatility();
    return {log_price_ratio(context.spot_price(), option.lower_barrier()),
            log_price_ratio(option.upper_barrier(), option.lower_barrier()), std::log(option.lower_barrier()), sigma,
            model.risk_free_rate() - model.dividend_yield() - 0.5 * sigma * sigma, time};
}

template <typename Evaluate>
Result<double> integrate_exit(const Evaluate& evaluate, double left, double right,
                              std::array<double, 3> values, double estimate, double tolerance, int depth)
{
    const double center = std::midpoint(left, right);
    const auto low = evaluate(std::midpoint(left, center));
    if (!low) return std::unexpected(low.error());
    const auto high = evaluate(std::midpoint(center, right));
    if (!high) return std::unexpected(high.error());
    const double first = (center - left) * (values[0] + 4.0 * *low + values[1]) / 6.0;
    const double second = (right - center) * (values[1] + 4.0 * *high + values[2]) / 6.0;
    const double refined = first + second;
    if (std::abs(refined - estimate) <= 15.0 * tolerance || center == left || center == right)
        return refined + (refined - estimate) / 15.0;
    if (depth == 0)
        return std::unexpected(Error{ErrorCategory::invalid_result, "double-barrier first-exit integral did not converge"});
    const auto a = integrate_exit(evaluate, left, center, std::array{values[0], *low, values[1]}, first, tolerance / 2.0, depth - 1);
    if (!a) return std::unexpected(a.error());
    const auto b = integrate_exit(evaluate, center, right, std::array{values[1], *high, values[2]}, second, tolerance / 2.0, depth - 1);
    if (!b) return std::unexpected(b.error());
    return *a + *b;
}

Result<double> price_double_barrier(const DoubleBarrierOption& option, const PricingContext& context)
{
    const auto life = validate_valuation_within_instrument_life(context.valuation_time(), option.effective_date(), option.expiry_date());
    if (!life) return std::unexpected(life.error());
    if (option.observation_mode() == ObservationMode::scheduled) {
        const auto valid = validate_observation_dates(option.observation_dates(), option.effective_date(), option.expiry_date(), context.calendar());
        if (!valid) return std::unexpected(valid.error());
    }
    if (const auto settled = resolved_barrier_price(option, context)) return *settled;
    if (option.observation_mode() == ObservationMode::scheduled)
        return std::unexpected(Error{ErrorCategory::unsupported_operation, "analytical double-barrier pricing requires continuous monitoring"});
    const double time = actual_365_fixed_year_fraction(context.valuation_time(), option.expiry_date());
    const auto density = distribution(option, context, time);
    const double discount = -context.model_parameters().risk_free_rate() * time;
    const bool call = option.option_type() == OptionType::call;
    const double strike = log_price_ratio(option.strike(), option.lower_barrier());
    const double lower = call ? std::clamp(strike, 0.0, density.band) : 0.0;
    const double upper = call ? density.band : std::clamp(strike, 0.0, density.band);
    const auto cash = density.moment(0, lower, upper, discount);
    if (!cash) return std::unexpected(cash.error());
    const auto asset = density.moment(1, lower, upper, discount);
    if (!asset) return std::unexpected(asset.error());
    double value = (call ? 1.0 : -1.0) * (*asset - option.strike() * *cash);
    if (option.barrier_terms().is_knock_in()) {
        const auto vanilla = price_at_volatility(*make_european_option(option.option_type(), option.strike(), option.effective_date(), option.expiry_date()),
                                                 context, context.model_parameters().volatility(), GreeksRequest{});
        if (!vanilla) return std::unexpected(vanilla.error());
        value = vanilla->price() - value;
    }
    if (option.rebate() != 0.0) {
        if (option.rebate_timing() == RebateTiming::at_hit && !option.barrier_terms().is_knock_in()) {
            const auto hit = double_barrier_hit_value(option, context);
            if (!hit) return std::unexpected(hit.error());
            value += *hit;
        } else {
            const auto probability = density.moment(0, 0.0, density.band, discount, !option.barrier_terms().is_knock_in());
            if (!probability) return std::unexpected(probability.error());
            value += option.rebate() * *probability;
        }
    }
    if (!std::isfinite(value) || value < -1e-11 * std::max({1.0, context.spot_price(), option.strike(), option.rebate()}))
        return std::unexpected(Error{ErrorCategory::invalid_result, "analytical double-barrier price is not representable"});
    return std::max(0.0, value);
}
} // namespace

Result<double> detail::double_barrier_hit_value(const DoubleBarrierOption& option, const PricingContext& context)
{
    const double time = actual_365_fixed_year_fraction(context.valuation_time(), option.expiry_date());
    const auto density = distribution(option, context, time);
    const double rate = context.model_parameters().risk_free_rate();
    const auto exit = density.moment(0, 0.0, density.band, 0.0, true);
    if (!exit) return std::unexpected(exit.error());
    double value = scaled_exponential(*exit, -rate * time);
    if (rate != 0.0) {
        // Split near diffusion and drift exit scales so a short boundary layer cannot be missed.
        std::vector<double> breaks{0.0, time};
        const auto append = [&](double t) {
            if (t > 0.0 && t < time) breaks.push_back(t);
        };
        for (const double distance : {density.initial, density.band - density.initial})
            for (const double factor : {0.0625, 1.0, 16.0})
                append(factor * (distance / density.volatility) * (distance / density.volatility));
        if (density.drift != 0.0) {
            const double hit = density.drift > 0.0 ? (density.band - density.initial) / density.drift : -density.initial / density.drift;
            const double width = density.volatility * std::sqrt(hit) / std::abs(density.drift);
            append(hit);
            for (const double factor : {-12.0, -4.0, -1.0, 1.0, 4.0, 12.0})
                append(hit + factor * width);
        }
        std::ranges::sort(breaks);
        breaks.erase(std::ranges::unique(breaks).begin(), breaks.end());
        const auto evaluate = [&](double t) -> Result<double> {
            if (t == 0.0) return 0.0;
            auto shifted = density;
            shifted.time = t;
            return shifted.moment(0, 0.0, density.band, -rate * t, true);
        };
        quadrature::Sum integral;
        for (std::size_t index = 1; index < breaks.size(); ++index) {
            const double left = breaks[index - 1], right = breaks[index];
            const auto a = evaluate(left), b = evaluate(std::midpoint(left, right)), c = evaluate(right);
            if (!a) return std::unexpected(a.error());
            if (!b) return std::unexpected(b.error());
            if (!c) return std::unexpected(c.error());
            const auto part = integrate_exit(evaluate, left, right, std::array{*a, *b, *c},
                                             (right - left) * (*a + 4.0 * *b + *c) / 6.0,
                                             1e-13 * std::max(1.0, std::exp(-rate * time)) / static_cast<double>(breaks.size()), 32);
            if (!part) return std::unexpected(part.error());
            integral.add(*part);
        }
        value += rate * integral.total();
    }
    if (!std::isfinite(value) || value < -1e-12)
        return std::unexpected(Error{ErrorCategory::invalid_result, "double-barrier first-exit value is not representable"});
    return checked_price(option.rebate() * std::max(0.0, value));
}

Result<double> AnalyticDoubleBarrierEngine::price(const DoubleBarrierOption& option, const PricingContext& context) const
{
    return price_double_barrier(option, context);
}

Result<PricingResult> AnalyticDoubleBarrierEngine::price_with_greeks(const DoubleBarrierOption& option, const PricingContext& context,
                                                                     GreeksRequest greeks, NumericalShiftSettings settings) const
{
    return detail::price_with_greeks(*this, option, context, greeks, settings);
}
} // namespace kiyosi
