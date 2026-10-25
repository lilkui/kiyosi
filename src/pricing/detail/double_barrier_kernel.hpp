#pragma once

#include <cmath>
#include <numbers>

#if defined(__CUDACC__)
#define KIYOSI_DOUBLE_BARRIER_HOST_DEVICE __host__ __device__
#else
#define KIYOSI_DOUBLE_BARRIER_HOST_DEVICE
#endif

namespace kiyosi::detail {

struct DoubleBarrierSurvival {
    double value{};
    double first{};
    double second{};
};

// Drift cancels in the ratio of absorbing and free transition densities.
// Derivatives hold the terminal log asset fixed and differentiate the initial log asset.
KIYOSI_DOUBLE_BARRIER_HOST_DEVICE inline DoubleBarrierSurvival double_barrier_survival(
    double initial, double terminal, double band, double deviation, bool derivatives)
{
    if (terminal <= 0.0 || terminal >= band) return {};
    double x = initial / band, y = terminal / band;
    const double ratio = deviation / band;
    const double variance = ratio * ratio;
    if (variance == 0.0) return {1.0};
    if (!(variance > 0.0) || variance == HUGE_VAL) return {HUGE_VAL};
    const bool reflected = x + y > 1.0;
    if (reflected) {
        x = 1.0 - x;
        y = 1.0 - y;
    }
    constexpr double pi = std::numbers::pi;
    DoubleBarrierSurvival sum;
    if (variance < 0.25) {
        sum.value = -std::expm1(-2.0 * (x / ratio) * (y / ratio));
        const auto add = [&](double k, bool direct) {
            const double exponent = direct ? -2.0 * (k / ratio) * ((y - x + k) / ratio)
                                           : -2.0 * ((x + k) / ratio) * ((y + k) / ratio);
            const double sign = direct ? 1.0 : -1.0;
            const double weight = std::exp(exponent);
            if (k != 0.0) sum.value += sign * weight;
            if (derivatives) {
                const double numerator = direct ? 2.0 * k : -2.0 * (y + k);
                if (numerator != 0.0) {
                    const double log_slope = std::log(std::abs(numerator)) - 2.0 * std::log(ratio);
                    sum.first += sign * std::copysign(std::exp(exponent + log_slope), numerator);
                    sum.second += sign * std::exp(exponent + 2.0 * log_slope);
                }
            }
            return weight;
        };
        add(0.0, false);
        bool converged = false;
        for (int n = 1; n <= 64; ++n) {
            const double k = n;
            const double tail = add(k, true) + add(k, false) + add(-k, true) + add(-k, false);
            if (n >= 2 && tail < 1e-18) {
                converged = true;
                break;
            }
        }
        if (!converged) return {HUGE_VAL};
    } else {
        const double distance = y - x;
        const double log_scale = std::log(2.0 * std::sqrt(2.0 * pi) * ratio) + 0.5 * distance * distance / variance;
        bool converged = false;
        for (int n = 1; n <= 256; ++n) {
            const double frequency = n * pi;
            const double exponent = log_scale - 0.5 * frequency * frequency * variance;
            const double weight = std::exp(exponent);
            const double sy = std::sin(frequency * y);
            const double product = std::sin(frequency * x) * sy;
            sum.value += weight * product;
            if (derivatives) {
                const double first = frequency * std::cos(frequency * x) * sy;
                const double score = -distance / variance;
                sum.first += weight * (first + score * product);
                sum.second += weight * (2.0 * score * first +
                                        (score * score + 1.0 / variance - frequency * frequency) * product);
            }
            if (n >= 4 && exponent + 4.0 * std::log(frequency) < -40.0) {
                converged = true;
                break;
            }
        }
        if (!converged) return {HUGE_VAL};
    }
    const bool bounded = sum.value >= -1e-13 && sum.value <= 1.0 + 1e-13;
    if (!bounded) return {HUGE_VAL};
    sum.value = std::fmin(1.0, std::fmax(0.0, sum.value));
    if (derivatives) {
        sum.first = (reflected ? -sum.first : sum.first) / band;
        sum.second = (sum.second / band) / band;
    }
    return sum;
}

} // namespace kiyosi::detail

#undef KIYOSI_DOUBLE_BARRIER_HOST_DEVICE
