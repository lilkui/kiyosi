#pragma once

#include <type_traits>

#include <kiyosi/instruments/digital.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>
#include <kiyosi/pricing/settings/monte_carlo.hpp>
#include <kiyosi/pricing/settings/numerical_shift.hpp>

namespace kiyosi {

class KIYOSI_EXPORT MonteCarloDigitalEngine;
[[nodiscard]] Result<MonteCarloDigitalEngine> make_monte_carlo_digital_engine(MonteCarloSettings settings);

/// Direct exact-terminal Monte Carlo valuation of European cash-or-nothing and asset-or-nothing options.
/// Step count is validated but unused. CPU supports pseudorandom and scrambled Sobol sampling;
/// optional CUDA supports pseudorandom sampling. CPU and CUDA streams differ.
/// Delta and gamma use likelihood-ratio estimates from the same draws; other Greeks are unavailable.
/// Estimates have sampling error, especially near expiry. Numerical shifts are validated but unused.
/// Concurrent const calls on shared or distinct engines are safe without concurrent mutation;
/// all sampling and workspace storage is request-local. Operational failures follow MonteCarloSettings.
class KIYOSI_EXPORT MonteCarloDigitalEngine {
public:
    MonteCarloDigitalEngine() = default;

    [[nodiscard]] Result<double> price(const CashOrNothingOption& option, const PricingContext& context) const;
    [[nodiscard]] Result<double> price(const AssetOrNothingOption& option, const PricingContext& context) const;

    /// Returns price and requested supported Greeks; all Greeks are unavailable at expiry.
    [[nodiscard]] Result<PricingResult> price_with_greeks(const CashOrNothingOption& option, const PricingContext& context,
                                                          GreeksRequest greeks, NumericalShiftSettings settings = {}) const;
    [[nodiscard]] Result<PricingResult> price_with_greeks(const AssetOrNothingOption& option, const PricingContext& context,
                                                          GreeksRequest greeks, NumericalShiftSettings settings = {}) const;

    [[nodiscard]] MonteCarloSettings settings() const noexcept { return settings_; }

private:
    explicit MonteCarloDigitalEngine(MonteCarloSettings settings) : settings_(settings) {}
    friend Result<MonteCarloDigitalEngine> make_monte_carlo_digital_engine(MonteCarloSettings);
    friend Result<MonteCarloDigitalEngine> make_engine(MonteCarloSettings settings, std::type_identity<MonteCarloDigitalEngine>)
    {
        return make_monte_carlo_digital_engine(settings);
    }
    MonteCarloSettings settings_;
};

/// Validates settings; returns invalid_parameter, or unsupported_operation for CUDA with Sobol.
[[nodiscard]] inline Result<MonteCarloDigitalEngine> make_monte_carlo_digital_engine(MonteCarloSettings settings)
{
    const auto valid = detail::validate_monte_carlo_settings(settings);
    if (!valid) return std::unexpected(valid.error());
    return MonteCarloDigitalEngine{settings};
}

namespace detail {
template <typename Option>
inline constexpr bool is_builtin_monte_carlo_engine<MonteCarloDigitalEngine, Option> = true;
} // namespace detail

} // namespace kiyosi
