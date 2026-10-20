#pragma once

#include <type_traits>

#include <kiyosi/pricing/settings/implied.hpp>

#include <kiyosi/instruments/autocallable/phoenix.hpp>
#include <kiyosi/instruments/autocallable/snowball.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>
#include <kiyosi/pricing/settings/numerical_shift.hpp>
#include <kiyosi/pricing/settings/finite_difference.hpp>

namespace kiyosi {

class FiniteDifferenceAutocallableEngine;
[[nodiscard]] Result<FiniteDifferenceAutocallableEngine> make_finite_difference_autocallable_engine(FiniteDifferenceSettings settings);

/// One- or two-layer backward induction, depending on whether the note has knock-in state, with
/// knock-out and coupon events anchored onto the time grid.
/// Uses concentrated contract-level grids, projected future events and Crank-Nicolson
/// with four implicit-Euler half steps after non-smooth events. Explicit Euler retains
/// uniform/sinh grids and the corresponding stability checks. Defaults are 800 asset
/// and 2,000 time steps; they target the documented fixed accuracy scenarios, not all contracts.
class FiniteDifferenceAutocallableEngine {
public:
    /// Creates an engine with valid default settings.
    FiniteDifferenceAutocallableEngine() = default;

    /// Prices an autocallable note by finite differences.
    /// @return Price, or a contract, context, or numerical error.
    template <AutocallableProduct Note>
    [[nodiscard]] KIYOSI_EXPORT Result<double> price(const Note& note, const PricingContext& context) const;

    /// Prices with the explicitly requested Greeks; unavailable measures remain empty.
    /// Delta, gamma, and speed reuse the solved layer; vanna/zomma and charm/color difference native delta/gamma shifted in volatility and time.
    /// Other measures use numerical price differences.
    template <AutocallableProduct Note>
    [[nodiscard]] KIYOSI_EXPORT Result<PricingResult> price_with_greeks(const Note& option, const PricingContext& context,
                                                                        GreeksRequest greeks, NumericalShiftSettings settings = {}) const;

    /// Returns the engine settings.
    FiniteDifferenceSettings settings() const noexcept { return settings_; }

private:
    explicit FiniteDifferenceAutocallableEngine(FiniteDifferenceSettings settings) : settings_(settings) {}
    friend Result<FiniteDifferenceAutocallableEngine> make_finite_difference_autocallable_engine(FiniteDifferenceSettings);
    friend Result<FiniteDifferenceAutocallableEngine> make_engine(FiniteDifferenceSettings settings, std::type_identity<FiniteDifferenceAutocallableEngine>)
    {
        return make_finite_difference_autocallable_engine(settings);
    }

    FiniteDifferenceSettings settings_{800, 2000};
};

/// Creates an engine after validating its configuration.
/// @return The engine, or an `invalid_parameter` error.
[[nodiscard]] inline Result<FiniteDifferenceAutocallableEngine> make_finite_difference_autocallable_engine(FiniteDifferenceSettings settings)
{
    const auto valid = validate_finite_difference_settings(settings);
    if (!valid) return std::unexpected(valid.error());
    return FiniteDifferenceAutocallableEngine{settings};
}

namespace detail {
template <AutocallableProduct Note>
inline constexpr bool is_builtin_finite_difference_engine<FiniteDifferenceAutocallableEngine, Note> = true;
template <AutocallableProduct Note>
inline constexpr bool has_affine_coupon_price<FiniteDifferenceAutocallableEngine, Note> = true;
} // namespace detail

} // namespace kiyosi
