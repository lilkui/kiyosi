#pragma once

#include <kiyosi/instruments/accumulator.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>
#include <kiyosi/pricing/settings/accumulator_quadrature.hpp>
#include <kiyosi/pricing/settings/numerical_shift.hpp>

namespace kiyosi {

class QuadratureAccumulatorEngine;
[[nodiscard]] Result<QuadratureAccumulatorEngine> make_quadrature_accumulator_engine(AccumulatorQuadratureSettings settings);

/// Event-date BSM integration of the two affine accumulated-quantity layers.
/// Splits contract events exactly and integrates cash/asset settlement pieces directly.
/// Finite-domain continuation tails use the contract's affine low-price limits.
/// Defaults target documented scenarios, not arbitrary-contract error bounds; check space and
/// integration-domain convergence separately. Shared finite-bump Greek conventions apply.
/// Concurrent const calls on distinct or shared instances are safe; workspaces are request-local.
/// Calls are synchronous with no cancellation; allocation failures propagate std::bad_alloc.
class QuadratureAccumulatorEngine {
public:
    QuadratureAccumulatorEngine() = default;

    [[nodiscard]] KIYOSI_EXPORT Result<double> price(const Accumulator& option, const PricingContext& context) const;

    [[nodiscard]] KIYOSI_EXPORT Result<PricingResult> price_with_greeks(
        const Accumulator& option, const PricingContext& context, GreeksRequest greeks, NumericalShiftSettings settings = {}) const;

    AccumulatorQuadratureSettings settings() const noexcept { return settings_; }

private:
    explicit QuadratureAccumulatorEngine(AccumulatorQuadratureSettings settings) : settings_(settings) {}
    friend Result<QuadratureAccumulatorEngine> make_quadrature_accumulator_engine(AccumulatorQuadratureSettings);
    AccumulatorQuadratureSettings settings_{};
};

[[nodiscard]] inline Result<QuadratureAccumulatorEngine> make_quadrature_accumulator_engine(AccumulatorQuadratureSettings settings)
{
    const auto valid = validate_accumulator_quadrature_settings(settings);
    if (!valid) return std::unexpected(valid.error());
    return QuadratureAccumulatorEngine{settings};
}

} // namespace kiyosi
