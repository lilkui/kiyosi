#pragma once

#include <kiyosi/instruments/barrier/option.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>
#include <kiyosi/pricing/settings/monte_carlo.hpp>
#include <kiyosi/pricing/settings/numerical_shift.hpp>

namespace kiyosi {

class KIYOSI_EXPORT MonteCarloBarrierEngine;
[[nodiscard]] Result<MonteCarloBarrierEngine> make_monte_carlo_barrier_engine(MonteCarloSettings settings);

/// European single-barrier Monte Carlo under constant-coefficient BSM dynamics.
/// Continuous monitoring uses exact terminal sampling and conditional survival weights;
/// scheduled monitoring samples the original observation instants and expiry. Step count
/// is validated but unused. CPU supports pseudorandom/Sobol sampling and optional CUDA
/// supports pseudorandom sampling, with the concurrency and failure contract of MonteCarloSettings.
/// Delta/gamma use absorbing-density derivatives or first-observation likelihood scores;
/// continuous at-hit rebate derivatives use deterministic numerical shifts. Other Greeks
/// are unavailable. Estimates have sampling error, especially near an imminent observation.
/// All workspace is request-local; concurrent const calls on shared or distinct engines
/// are safe without concurrent mutation. There is no cancellation interface.
class KIYOSI_EXPORT MonteCarloBarrierEngine {
public:
    MonteCarloBarrierEngine() = default;

    [[nodiscard]] Result<double> price(const BarrierOption& option, const PricingContext& context) const;
    [[nodiscard]] Result<PricingResult> price_with_greeks(const BarrierOption& option, const PricingContext& context,
                                                          GreeksRequest greeks, NumericalShiftSettings settings = {}) const;
    [[nodiscard]] MonteCarloSettings settings() const noexcept { return settings_; }

private:
    explicit MonteCarloBarrierEngine(MonteCarloSettings settings) : settings_(settings) {}
    friend Result<MonteCarloBarrierEngine> make_monte_carlo_barrier_engine(MonteCarloSettings);
    friend Result<MonteCarloBarrierEngine> make_engine(MonteCarloSettings settings, std::type_identity<MonteCarloBarrierEngine>)
    {
        return make_monte_carlo_barrier_engine(settings);
    }
    MonteCarloSettings settings_;
};

[[nodiscard]] inline Result<MonteCarloBarrierEngine> make_monte_carlo_barrier_engine(MonteCarloSettings settings)
{
    const auto valid = detail::validate_monte_carlo_settings(settings);
    if (!valid) return std::unexpected(valid.error());
    return MonteCarloBarrierEngine{settings};
}

namespace detail {
template <typename Option>
inline constexpr bool is_builtin_monte_carlo_engine<MonteCarloBarrierEngine, Option> = true;
} // namespace detail
} // namespace kiyosi
