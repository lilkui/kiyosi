#pragma once

#include <type_traits>

#include <kiyosi/instruments/range_accrual.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>
#include <kiyosi/pricing/settings/monte_carlo.hpp>
#include <kiyosi/pricing/settings/numerical_shift.hpp>

namespace kiyosi {

class KIYOSI_EXPORT MonteCarloRangeAccrualEngine;
[[nodiscard]] Result<MonteCarloRangeAccrualEngine> make_monte_carlo_range_accrual_engine(TradingDayMonteCarloSettings settings);

/// Exact BSM transitions between observations, with conditional-probability smoothing.
/// Supports deterministic rate/dividend curves, CPU/CUDA and CPU Sobol. Current observations
/// use spot; earlier observations require explicit history. Pre-effective valuations are valid.
/// No standard error or confidence interval is reported. Operational failures and concurrency
/// follow TradingDayMonteCarloSettings: concurrent const calls are safe without mutation.
class KIYOSI_EXPORT MonteCarloRangeAccrualEngine {
public:
    MonteCarloRangeAccrualEngine() = default;
    [[nodiscard]] Result<double> price(const RangeAccrualOption&, const PricingContext&) const;
    /// Uses common random samples for finite differences. At expiry all Greeks are unavailable;
    /// time stencils touching observations and spot stencils crossing current bounds are unavailable.
    [[nodiscard]] Result<PricingResult> price_with_greeks(const RangeAccrualOption&, const PricingContext&,
                                                          GreeksRequest, NumericalShiftSettings = {}) const;
    TradingDayMonteCarloSettings settings() const noexcept { return settings_; }

private:
    explicit MonteCarloRangeAccrualEngine(TradingDayMonteCarloSettings settings) : settings_(settings) {}
    TradingDayMonteCarloSettings settings_;
    friend Result<MonteCarloRangeAccrualEngine> make_monte_carlo_range_accrual_engine(TradingDayMonteCarloSettings);
    friend Result<MonteCarloRangeAccrualEngine> make_engine(TradingDayMonteCarloSettings settings, std::type_identity<MonteCarloRangeAccrualEngine>)
    {
        return make_monte_carlo_range_accrual_engine(settings);
    }
};

[[nodiscard]] inline Result<MonteCarloRangeAccrualEngine> make_monte_carlo_range_accrual_engine(TradingDayMonteCarloSettings settings)
{
    const auto valid = detail::validate_monte_carlo_settings(settings);
    if (!valid) return std::unexpected(valid.error());
    return MonteCarloRangeAccrualEngine{settings};
}

namespace detail {
template <typename Option>
inline constexpr bool is_builtin_monte_carlo_engine<MonteCarloRangeAccrualEngine, Option> = true;
}
} // namespace kiyosi
