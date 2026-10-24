#pragma once

#include <type_traits>

#include <cstdint>
#include <optional>

#include <kiyosi/instruments/accumulator.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>
#include <kiyosi/pricing/settings/numerical_shift.hpp>
#include <kiyosi/pricing/settings/monte_carlo.hpp>

namespace kiyosi {

class KIYOSI_EXPORT MonteCarloAccumulatorEngine;
[[nodiscard]] Result<MonteCarloAccumulatorEngine> make_monte_carlo_accumulator_engine(TradingDayMonteCarloSettings settings);

/// Simulates the trading-day accrual, terminating each path at the knock-out level.
/// Accepts 1..10,000,000 paths.
class KIYOSI_EXPORT MonteCarloAccumulatorEngine {
public:
    /// Creates an engine with valid default settings.
    MonteCarloAccumulatorEngine() = default;

    /// Prices an accumulator by Monte Carlo simulation.
    /// @return Price, or a contract, context, numerical, or backend error.
    [[nodiscard]] Result<double> price(const Accumulator& option, const PricingContext& context) const;

    /// Prices with the explicitly requested Greeks; unavailable measures remain empty.
    /// CPU spot bumps share per-call random draws and growth factors with per-block reduction storage.
    [[nodiscard]] Result<PricingResult> price_with_greeks(const Accumulator& option, const PricingContext& context,
                                                          GreeksRequest greeks, NumericalShiftSettings settings = {}) const;

    /// Returns the engine settings.
    TradingDayMonteCarloSettings settings() const noexcept { return settings_; }

private:
    explicit MonteCarloAccumulatorEngine(TradingDayMonteCarloSettings settings) : settings_(settings) {}
    friend Result<MonteCarloAccumulatorEngine> make_monte_carlo_accumulator_engine(TradingDayMonteCarloSettings);
    friend Result<MonteCarloAccumulatorEngine> make_engine(TradingDayMonteCarloSettings settings, std::type_identity<MonteCarloAccumulatorEngine>)
    {
        return make_monte_carlo_accumulator_engine(settings);
    }

    TradingDayMonteCarloSettings settings_;
};

/// Creates an engine after validating its configuration.
/// @return The engine, an `invalid_parameter` error, or `unsupported_operation` for CUDA with Sobol.
[[nodiscard]] inline Result<MonteCarloAccumulatorEngine> make_monte_carlo_accumulator_engine(TradingDayMonteCarloSettings settings)
{
    const auto valid = detail::validate_monte_carlo_settings(settings);
    if (!valid) return std::unexpected(valid.error());
    return MonteCarloAccumulatorEngine{settings};
}

namespace detail {
template <typename Option>
inline constexpr bool is_builtin_monte_carlo_engine<MonteCarloAccumulatorEngine, Option> = true;
} // namespace detail

} // namespace kiyosi
