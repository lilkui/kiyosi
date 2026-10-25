#pragma once

#include <kiyosi/instruments/barrier/double.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>
#include <kiyosi/pricing/settings/monte_carlo.hpp>
#include <kiyosi/pricing/settings/numerical_shift.hpp>

namespace kiyosi {

class KIYOSI_EXPORT MonteCarloDoubleBarrierEngine;
[[nodiscard]] Result<MonteCarloDoubleBarrierEngine> make_monte_carlo_double_barrier_engine(MonteCarloSettings settings);

/// European double-barrier Monte Carlo under constant-coefficient BSM dynamics.
/// Continuous monitoring uses terminal conditional survival weights and a separately
/// evaluated discounted first-exit rebate. Scheduled monitoring uses original observation
/// dates and expiry, including an early monitoring end. Step count is validated but unused.
/// CPU supports pseudorandom/Sobol sampling; optional CUDA supports pseudorandom sampling.
/// Delta/gamma use survival-density derivatives or first-observation likelihood scores;
/// continuous at-hit rebate derivatives use deterministic numerical shifts. Other Greeks
/// are unavailable. Sampling error can be large near an imminent scheduled observation.
/// All workspace is request-local. Concurrent const calls on shared or distinct engines
/// are safe without concurrent assignment, with the failure contract of MonteCarloSettings.
/// No cancellation interface is provided.
class KIYOSI_EXPORT MonteCarloDoubleBarrierEngine {
public:
    MonteCarloDoubleBarrierEngine() = default;
    [[nodiscard]] Result<double> price(const DoubleBarrierOption& option, const PricingContext& context) const;
    [[nodiscard]] Result<PricingResult> price_with_greeks(const DoubleBarrierOption& option, const PricingContext& context,
                                                          GreeksRequest greeks, NumericalShiftSettings settings = {}) const;
    [[nodiscard]] MonteCarloSettings settings() const noexcept { return settings_; }

private:
    explicit MonteCarloDoubleBarrierEngine(MonteCarloSettings settings) : settings_(settings) {}
    friend Result<MonteCarloDoubleBarrierEngine> make_monte_carlo_double_barrier_engine(MonteCarloSettings);
    friend Result<MonteCarloDoubleBarrierEngine> make_engine(MonteCarloSettings settings, std::type_identity<MonteCarloDoubleBarrierEngine>)
    {
        return make_monte_carlo_double_barrier_engine(settings);
    }
    MonteCarloSettings settings_;
};

[[nodiscard]] inline Result<MonteCarloDoubleBarrierEngine> make_monte_carlo_double_barrier_engine(MonteCarloSettings settings)
{
    const auto valid = detail::validate_monte_carlo_settings(settings);
    if (!valid) return std::unexpected(valid.error());
    return MonteCarloDoubleBarrierEngine{settings};
}

namespace detail {
template <typename Option>
inline constexpr bool is_builtin_monte_carlo_engine<MonteCarloDoubleBarrierEngine, Option> = true;
} // namespace detail
} // namespace kiyosi
