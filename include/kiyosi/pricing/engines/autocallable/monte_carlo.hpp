#pragma once

#include <type_traits>

#include <array>

#include <kiyosi/pricing/settings/implied.hpp>
#include <cstdint>
#include <optional>

#include <kiyosi/instruments/autocallable/phoenix.hpp>
#include <kiyosi/instruments/autocallable/snowball.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>
#include <kiyosi/pricing/detail/instrument_analytics.hpp>
#include <kiyosi/pricing/settings/numerical_shift.hpp>
#include <kiyosi/pricing/settings/monte_carlo.hpp>

namespace kiyosi {

namespace detail {
// Coupon variants share asset paths; the implied solver supplies otherwise identical notes.
template <AutocallableProduct Note>
[[nodiscard]] KIYOSI_EXPORT Result<std::array<double, 2>> monte_carlo_coupon_prices(
    const Note& lower, const Note& upper, const PricingContext& context,
    const TradingDayMonteCarloSettings& settings);
} // namespace detail

class MonteCarloAutocallableEngine;
[[nodiscard]] Result<MonteCarloAutocallableEngine> make_monte_carlo_autocallable_engine(TradingDayMonteCarloSettings settings);

/// Simulates observation dates and expiry, adding trading days for daily knock-in monitoring.
/// Accepts 1..10,000,000 paths.
class MonteCarloAutocallableEngine {
public:
    /// Creates an engine with valid default settings.
    MonteCarloAutocallableEngine() = default;

    /// Prices an autocallable note by Monte Carlo simulation.
    /// @return Price, or a contract, context, numerical, or backend error.
    template <AutocallableProduct Note>
    [[nodiscard]] KIYOSI_EXPORT Result<double> price(const Note& note, const PricingContext& context) const;

    /// Prices with the explicitly requested Greeks; unavailable measures remain empty.
    /// CPU spot sensitivities share per-path draws with separate event states and per-block reduction storage.
    template <AutocallableProduct Note>
    [[nodiscard]] KIYOSI_EXPORT Result<PricingResult> price_with_greeks(const Note& option, const PricingContext& context,
                                                                        GreeksRequest greeks, NumericalShiftSettings settings = {}) const;

    /// Returns the engine settings.
    TradingDayMonteCarloSettings settings() const noexcept { return settings_; }

private:
    explicit MonteCarloAutocallableEngine(TradingDayMonteCarloSettings settings) : settings_(settings) {}
    friend Result<MonteCarloAutocallableEngine> make_monte_carlo_autocallable_engine(TradingDayMonteCarloSettings);
    friend Result<MonteCarloAutocallableEngine> make_engine(TradingDayMonteCarloSettings settings, std::type_identity<MonteCarloAutocallableEngine>)
    {
        return make_monte_carlo_autocallable_engine(settings);
    }

    TradingDayMonteCarloSettings settings_;
};

/// Creates an engine after validating its configuration.
/// @return The engine, or an `invalid_parameter` error.
[[nodiscard]] inline Result<MonteCarloAutocallableEngine> make_monte_carlo_autocallable_engine(TradingDayMonteCarloSettings settings)
{
    const auto valid = detail::validate_monte_carlo_settings(settings);
    if (!valid) return std::unexpected(valid.error());
    return MonteCarloAutocallableEngine{settings};
}

namespace detail {
template <AutocallableProduct Note>
inline constexpr bool is_builtin_monte_carlo_engine<MonteCarloAutocallableEngine, Note> = true;
template <AutocallableProduct Note>
inline constexpr bool has_affine_coupon_price<MonteCarloAutocallableEngine, Note> = true;
template <AutocallableProduct Note>
Result<std::array<double, 2>> coupon_endpoint_prices(
    const MonteCarloAutocallableEngine& engine, const Note& lower, const Note& upper, const PricingContext& context)
{
    return monte_carlo_coupon_prices(lower, upper, context, engine.settings());
}

} // namespace detail

} // namespace kiyosi
