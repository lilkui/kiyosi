#pragma once

#include <cfloat>
#include <cmath>

#include "monte_carlo_simulation.hpp"

#if defined(__CUDACC__)
#define KIYOSI_BARRIER_HOST_DEVICE __host__ __device__
#else
#define KIYOSI_BARRIER_HOST_DEVICE
#endif

namespace kiyosi::detail {

struct BarrierMonteCarloStep {
    SimulationStep simulation{};
    bool observe{};
};

struct BarrierMonteCarloSample {
    double price{};
    double delta{};
    double gamma{};
};

// Validated, owning scalar parameters shared by CPU and CUDA estimators.
struct BarrierMonteCarloParameters {
    double spot{};
    double strike{};
    double barrier{};
    double log_spot{};
    double initial_distance{};
    double rebate{};
    MonteCarloDiscount terminal_discount{};
    bool call{};
    bool up{};
    bool knock_in{};
    bool at_hit{};
    bool continuous{};

    KIYOSI_BARRIER_HOST_DEVICE bool breached(double asset) const
    {
        return up ? asset >= barrier : asset <= barrier;
    }

    KIYOSI_BARRIER_HOST_DEVICE double weighted(double log_weight, double amount) const
    {
        const double exponent = terminal_discount.exponent + log_weight;
        return MonteCarloDiscount{std::exp(exponent), exponent}.apply(amount);
    }

    KIYOSI_BARRIER_HOST_DEVICE BarrierMonteCarloSample sample(
        double log_asset, double first_normal, double first_width, bool touched, double knocked_out,
        bool delta, bool gamma) const
    {
        const double asset = std::exp(log_asset);
        const double intrinsic = call ? std::fmax(asset - strike, 0.0) : std::fmax(strike - asset, 0.0);
        double value = 0.0;
        double first = 0.0;
        double second = 0.0;
        if (continuous) {
            double log_survival = -HUGE_VAL;
            double exponent = 0.0;
            double slope = 0.0;
            if (!breached(asset)) {
                const double distance = up ? std::log(barrier) - log_asset : log_asset - std::log(barrier);
                slope = 2.0 * (distance / first_width) / first_width;
                exponent = -initial_distance * slope;
                log_survival = std::log(-std::expm1(exponent));
            }
            const double expiry_rebate = !knock_in && at_hit ? 0.0 : rebate;
            value = weighted(knock_in ? exponent : log_survival, intrinsic) +
                    weighted(knock_in ? log_survival : exponent, expiry_rebate);
            if ((delta || gamma) && slope > 0.0 && exponent > -HUGE_VAL) {
                const double orientation = knock_in ? -1.0 : 1.0;
                const double log_slope = std::log(slope);
                first = weighted(exponent + log_slope, orientation * (up ? -1.0 : 1.0) * (intrinsic - expiry_rebate));
                if (gamma) second = weighted(exponent + 2.0 * log_slope, -orientation * (intrinsic - expiry_rebate));
            }
        } else {
            value = touched && !knock_in ? knocked_out
                                         : terminal_discount.apply(touched == knock_in ? intrinsic : rebate);
        }
        BarrierMonteCarloSample result{value};
        if (delta || gamma) {
            const double score = first_normal / first_width;
            if (delta) result.delta = (value * score + first) / spot;
            if (gamma) {
                const double second_score = ((first_normal * first_normal - 1.0) / first_width - first_normal) / first_width;
                result.gamma = ((value * second_score + first * (2.0 * score - 1.0) + second) / spot) / spot;
            }
        }
        return result;
    }
};

} // namespace kiyosi::detail

#undef KIYOSI_BARRIER_HOST_DEVICE
