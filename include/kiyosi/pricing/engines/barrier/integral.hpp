#pragma once

#include <kiyosi/instruments/barrier/option.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>
#include <kiyosi/pricing/settings/numerical_shift.hpp>
#include <kiyosi/pricing/settings/quadrature.hpp>

namespace kiyosi {

class QuadratureBarrierEngine;
[[nodiscard]] Result<QuadratureBarrierEngine> make_quadrature_barrier_engine(QuadratureSettings settings);

/// BSM integration against the absorbing terminal density for continuous monitoring,
/// or event-date transitions for scheduled monitoring, including monitoring ending before expiry.
/// Continuous integration splits the payoff and barrier boundaries; scheduled integration
/// splits each barrier event and uses affine continuation tails outside its finite grid.
/// Check spatial and integration-domain convergence separately; settings imply no universal error bound.
/// Shared numerical Greek conventions and barrier boundary guards apply.
/// Concurrent const calls on shared or distinct instances are safe without concurrent assignment;
/// all workspaces are request-local. Calls complete synchronously, allocation and internal runtime
/// exceptions propagate, and no cancellation interface is provided. Parallel transition rows ignore
/// outer task cancellation so it cannot silently omit pricing work.
class QuadratureBarrierEngine {
public:
    QuadratureBarrierEngine() = default;

    [[nodiscard]] KIYOSI_EXPORT Result<double> price(const BarrierOption& option, const PricingContext& context) const;
    [[nodiscard]] KIYOSI_EXPORT Result<PricingResult> price_with_greeks(
        const BarrierOption& option, const PricingContext& context, GreeksRequest greeks,
        NumericalShiftSettings settings = {}) const;

    QuadratureSettings settings() const noexcept { return settings_; }

private:
    explicit QuadratureBarrierEngine(QuadratureSettings settings) : settings_(settings) {}
    friend Result<QuadratureBarrierEngine> make_quadrature_barrier_engine(QuadratureSettings);
    QuadratureSettings settings_{};
};

[[nodiscard]] inline Result<QuadratureBarrierEngine> make_quadrature_barrier_engine(QuadratureSettings settings)
{
    const auto valid = validate_quadrature_settings(settings);
    if (!valid) return std::unexpected(valid.error());
    return QuadratureBarrierEngine{settings};
}

} // namespace kiyosi
