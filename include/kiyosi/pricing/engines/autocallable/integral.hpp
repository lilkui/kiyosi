#pragma once

#include <kiyosi/instruments/autocallable/phoenix.hpp>
#include <kiyosi/instruments/autocallable/snowball.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>
#include <kiyosi/pricing/settings/autocallable_quadrature.hpp>
#include <kiyosi/pricing/settings/implied.hpp>
#include <kiyosi/pricing/settings/numerical_shift.hpp>

namespace kiyosi {

class QuadratureAutocallableEngine;
[[nodiscard]] Result<QuadratureAutocallableEngine> make_quadrature_autocallable_engine(AutocallableQuadratureSettings settings);

/// Event-date BSM integration of piecewise cubic smooth continuation, split at contract events.
/// Terminal cash/asset pieces are integrated directly. Finite-domain continuation tails use
/// contract limits; defaults target documented scenarios, not arbitrary-contract error bounds.
/// Concurrent const calls on distinct or shared instances are safe; workspaces are request-local.
/// Calls are synchronous with no cancellation; allocation failures propagate std::bad_alloc.
class QuadratureAutocallableEngine {
public:
    QuadratureAutocallableEngine() = default;

    template <AutocallableProduct Note>
    [[nodiscard]] KIYOSI_EXPORT Result<double> price(const Note& note, const PricingContext& context) const;

    /// Uses the shared finite-bump Greek conventions and unavailable-measure rules.
    template <AutocallableProduct Note>
    [[nodiscard]] KIYOSI_EXPORT Result<PricingResult> price_with_greeks(const Note& note, const PricingContext& context,
                                                                        GreeksRequest greeks, NumericalShiftSettings settings = {}) const;

    AutocallableQuadratureSettings settings() const noexcept { return settings_; }

private:
    explicit QuadratureAutocallableEngine(AutocallableQuadratureSettings settings) : settings_(settings) {}
    friend Result<QuadratureAutocallableEngine> make_quadrature_autocallable_engine(AutocallableQuadratureSettings);
    AutocallableQuadratureSettings settings_{};
};

[[nodiscard]] inline Result<QuadratureAutocallableEngine> make_quadrature_autocallable_engine(AutocallableQuadratureSettings settings)
{
    const auto valid = validate_autocallable_quadrature_settings(settings);
    if (!valid) return std::unexpected(valid.error());
    return QuadratureAutocallableEngine{settings};
}

namespace detail {
template <AutocallableProduct Note>
inline constexpr bool has_affine_coupon_price<QuadratureAutocallableEngine, Note> = true;
} // namespace detail
} // namespace kiyosi
