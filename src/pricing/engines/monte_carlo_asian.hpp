#pragma once

#include <cfloat>
#include <cmath>
#include <numbers>

#if defined(__CUDACC__)
#define KIYOSI_ASIAN_HOST_DEVICE __host__ __device__
#else
#define KIYOSI_ASIAN_HOST_DEVICE
#endif

namespace kiyosi::detail {

struct AsianMonteCarloSample {
    double price{};
    double delta{};
    double gamma{};
};

// CUDA cannot include the C++23 Result-based host math header.
KIYOSI_ASIAN_HOST_DEVICE inline double asian_log_cdf(double value)
{
    if (value >= -10.0) return std::log(0.5 * std::erfc(-value / std::numbers::sqrt2));
    const double inverse_square = (1.0 / value) / value;
    double term = 1.0, sum = 1.0;
    for (int i = 1; i <= 20; ++i) {
        term *= -(2.0 * i - 1.0) * inverse_square;
        sum += term;
    }
    return -0.5 * value * value - std::log(-value) + std::log(sum / std::sqrt(2.0 * std::numbers::pi));
}

KIYOSI_ASIAN_HOST_DEVICE inline double asian_log_add(double a, double b)
{
    const double high = std::fmax(a, b), low = std::fmin(a, b);
    return high == -HUGE_VAL ? high : high + std::log1p(std::exp(low - high));
}

KIYOSI_ASIAN_HOST_DEVICE inline double asian_positive_difference(double minuend, double subtrahend)
{
    if (minuend == HUGE_VAL) return HUGE_VAL;
    if (subtrahend == -HUGE_VAL) return std::exp(minuend);
    const double difference = -std::expm1(subtrahend - minuend);
    return difference > 0.0 ? std::exp(minuend + std::log(difference)) : 0.0;
}

// Owning, validated scalar inputs; no Python or mutable shared state.
struct AsianMonteCarloParameters {
    double spot{};
    double strike{};
    double realized{};
    double weight = 1.0;
    double log_discount{};
    double drift{};
    double width{};
    double log_width{};
    double lead_drift{};
    double lead_width{};
    double lead_width_ratio{};
    double interval_forward{};
    double initial_moneyness{};
    double standardized_moneyness{};
    double log_base_width{};
    int step_count{};
    bool call{};
    bool geometric{};
    bool lead{};

    KIYOSI_ASIAN_HOST_DEVICE AsianMonteCarloSample geometric_sample(double normal, bool delta, bool gamma) const
    {
        const double distance = initial_moneyness + drift + width * normal;
        const double standardized = standardized_moneyness + normal;
        if (!(call ? standardized > 0.0 : standardized < 0.0)) return {};
        const double sign = call ? 1.0 : -1.0;
        const double log_spot = std::log(spot), log_strike = std::log(strike);
        const double difference = std::abs(std::expm1(distance));
        const double log_payoff = std::abs(distance) < 1e-5 && std::abs(standardized) <= DBL_MAX
                                      ? log_strike + log_width + std::log(std::abs(standardized)) +
                                            (distance == 0.0 ? 0.0 : std::log(difference / std::abs(distance)))
                                      : (call ? log_strike + distance + std::log(-std::expm1(-distance))
                                              : log_strike + std::log(-std::expm1(distance)));
        AsianMonteCarloSample sample{std::exp(log_payoff + log_discount)};
        const double log_asset_weight = log_strike + distance + log_discount + std::log(weight);
        if (delta) sample.delta = sign * std::exp(log_asset_weight - log_spot);
        if (gamma) {
            const double numerator = normal - std::exp(log_base_width);
            if (numerator != 0.0)
                sample.gamma = sign * std::copysign(std::exp(log_asset_weight - 2.0 * log_spot +
                                                             std::log(std::abs(numerator)) - log_base_width),
                                                    numerator);
        }
        return sample;
    }

    KIYOSI_ASIAN_HOST_DEVICE AsianMonteCarloSample arithmetic_sample(
        double lead_growth, double relative_sum, double log_sum, double vanishing_center, bool delta, bool gamma) const
    {
        const double half = 0.5 / step_count, rest = 1.0 - half;
        const double log_spot = std::log(spot), log_weight = std::log(weight);
        const double log_rest = log_sum == -HUGE_VAL ? std::log(rest) + std::log1p(relative_sum / rest) : log_sum;
        const double log_q = log_spot + log_weight + lead_growth + log_rest + interval_forward;
        const double log_c = log_spot + log_weight + lead_growth + std::log(half);
        const double log_history = realized > 0.0 && weight < 1.0 ? std::log(realized) + std::log1p(-weight) : -HUGE_VAL;
        const double log_accrued = asian_log_add(log_history, log_c);
        const double log_strike = std::log(strike);
        if (log_accrued >= log_strike) {
            if (!call) return {};
            const double cash = asian_positive_difference(log_accrued + log_discount, log_strike + log_discount);
            return {std::exp(log_q + log_discount) + cash,
                    delta ? std::exp(log_q + log_discount - log_spot) + std::exp(log_c + log_discount - log_spot) : 0.0,
                    0.0};
        }
        const double log_b = log_strike + std::log(-std::expm1(log_accrued - log_strike));
        double forward = log_q - log_b;
        if (log_sum == -HUGE_VAL) {
            // Keep tiny path increments separate from the unit growth and from monetary scale.
            const double extra = std::expm1(lead_growth) + std::exp(lead_growth) *
                                                               (rest * std::expm1(interval_forward) + relative_sum * std::exp(interval_forward));
            const double excess = (1.0 - weight) * ((realized - strike) / spot) +
                                  weight * ((spot - strike) / spot) + weight * extra;
            const double b = std::exp(log_b - log_spot);
            if (b > 0.0 && std::abs(excess / b) < 0.5) forward = std::log1p(excess / b);
        }
        double center = width > 0.0 ? forward / width : forward == 0.0 ? vanishing_center
                                                                       : (forward / std::exp(log_width / 2.0)) / std::exp(log_width / 2.0);
        const double baseline = (1.0 - weight) * ((realized - strike) / spot) + weight * ((spot - strike) / spot);
        if (width < DBL_MIN && drift == 0.0 && lead_drift == 0.0 && interval_forward == 0.0 && baseline == 0.0)
            center = vanishing_center;
        const double d1 = center + 0.5 * width, d2 = center - 0.5 * width;
        const double sign = call ? 1.0 : -1.0;
        const double log_asset = log_q + log_discount + asian_log_cdf(sign * d1);
        const double log_cash = log_b + log_discount + asian_log_cdf(sign * d2);
        double price = call ? asian_positive_difference(log_asset, log_cash) : asian_positive_difference(log_cash, log_asset);
        if (width < 1e-5 && std::abs(center) < 10.0) {
            const double time_value = std::exp(log_q + log_discount + log_width - 0.5 * center * center -
                                               0.5 * std::log(2.0 * std::numbers::pi)) *
                                      (1.0 + (center * center - 1.0) * width * width / 24.0);
            const double correction = forward == 0.0 ? 0.0 : std::log(std::expm1(forward) / forward);
            const double intrinsic = center == 0.0 ? 0.0 : std::copysign(std::exp(log_cash + log_width + std::log(std::abs(center)) + correction), center);
            price = sign * intrinsic + time_value;
        } else if (width < 1e-5) {
            if (sign * center > 0.0) {
                price = width == 0.0 && forward == 0.0 ? std::exp(log_b + log_discount + log_width + std::log(std::abs(center))) : call ? std::exp(log_q + log_discount + std::log(-std::expm1(-forward)))
                                                                                                                                        : std::exp(log_b + log_discount + std::log(-std::expm1(forward)));
            } else {
                const double inverse_square = (1.0 / center) / center;
                double term = 1.0, sum = 1.0;
                for (int order = 1; order <= 20; ++order) {
                    term *= -(2.0 * order + 1.0) * inverse_square;
                    sum += term;
                }
                price = std::exp(log_q + log_discount - 0.5 * forward + log_width - 0.5 * center * center -
                                 0.5 * std::log(2.0 * std::numbers::pi) - 2.0 * std::log(std::abs(center)) + std::log(sum));
            }
        }
        AsianMonteCarloSample sample{price};
        if (delta)
            sample.delta = sign * (std::exp(log_c + log_discount - log_spot + asian_log_cdf(sign * d2)) +
                                   std::exp(log_q + log_discount - log_spot + asian_log_cdf(sign * d1)));
        if (gamma) {
            const double k = std::fma(-(1.0 - weight), realized, strike);
            sample.gamma = std::exp(log_discount + 2.0 * std::log(k) - 2.0 * log_spot - log_b - log_width -
                                    0.5 * d2 * d2 - 0.5 * std::log(2.0 * std::numbers::pi));
        }
        return sample;
    }

    template <typename Draw>
    KIYOSI_ASIAN_HOST_DEVICE AsianMonteCarloSample sample(Draw draw, bool delta, bool gamma) const
    {
        if (geometric) return geometric_sample(draw(), delta, gamma);
        const double lead_normal = lead ? draw() : 0.0;
        const double lead_growth = lead_drift + lead_width * lead_normal;
        double growth = 0.0, relative_sum = 0.0, log_sum = -HUGE_VAL;
        double normal_sum = 0.0, weighted_normals = 0.0;
        for (int step = 2; step <= step_count; ++step) {
            const double normal = draw();
            growth += drift + width * normal;
            const double fraction = (step == step_count ? 0.5 : 1.0) / step_count;
            if (width < DBL_MIN) {
                // Factoring out the width retains subnormal-volatility ATM time value.
                normal_sum += normal;
                weighted_normals += fraction * normal_sum;
            }
            if (growth > 600.0 || log_sum != -HUGE_VAL) {
                // Above exp(600), remaining unit offsets are below floating-point precision.
                if (log_sum == -HUGE_VAL) log_sum = std::log(1.0 - 0.5 / step_count + relative_sum);
                log_sum = asian_log_add(log_sum, std::log(fraction) + growth);
            } else {
                relative_sum += fraction * std::expm1(growth);
            }
        }
        const double vanishing_center = (weighted_normals + lead_width_ratio * lead_normal) / (1.0 - 0.5 / step_count);
        return arithmetic_sample(lead_growth, relative_sum, log_sum, vanishing_center, delta, gamma);
    }
};
} // namespace kiyosi::detail

#undef KIYOSI_ASIAN_HOST_DEVICE
