#pragma once

#include <type_traits>

#include <kiyosi/instruments/barrier/binary.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>
#include <kiyosi/pricing/settings/finite_difference.hpp>
#include <kiyosi/pricing/settings/numerical_shift.hpp>

namespace kiyosi {

class KIYOSI_EXPORT FiniteDifferenceBinaryBarrierEngine;
[[nodiscard]] Result<FiniteDifferenceBinaryBarrierEngine> make_finite_difference_binary_barrier_engine(FiniteDifferenceSettings);

/// Finite differences for cash/asset binary barriers and single touch/no-touch contracts.
/// Supports deterministic rate curves, continuous or scheduled monitoring, and the
/// instrument's settlement timing. Continuous monitoring fits the absorbing barrier;
/// scheduled observations average the event cell and restart implicit damping.
/// Concurrent const calls are safe without concurrent assignment to the engine.
class KIYOSI_EXPORT FiniteDifferenceBinaryBarrierEngine {
public:
    FiniteDifferenceBinaryBarrierEngine() = default;

    /// Returns price, or a contract, context, coverage, or numerical error.
    [[nodiscard]] Result<double> price(const BinaryBarrierOption&, const PricingContext&) const;
    /// Returns price, or a contract, context, coverage, or numerical error.
    [[nodiscard]] Result<double> price(const TouchOption&, const PricingContext&) const;
    /// Returns price and the requested available Greeks; unavailable measures remain empty.
    [[nodiscard]] Result<PricingResult> price_with_greeks(const BinaryBarrierOption&, const PricingContext&,
                                                          GreeksRequest, NumericalShiftSettings = {}) const;
    /// Returns price and the requested available Greeks; unavailable measures remain empty.
    [[nodiscard]] Result<PricingResult> price_with_greeks(const TouchOption&, const PricingContext&,
                                                          GreeksRequest, NumericalShiftSettings = {}) const;
    [[nodiscard]] FiniteDifferenceSettings settings() const noexcept { return settings_; }

private:
    explicit FiniteDifferenceBinaryBarrierEngine(FiniteDifferenceSettings settings) : settings_(settings) {}
    friend Result<FiniteDifferenceBinaryBarrierEngine> make_finite_difference_binary_barrier_engine(FiniteDifferenceSettings);
    friend Result<FiniteDifferenceBinaryBarrierEngine> make_engine(FiniteDifferenceSettings settings, std::type_identity<FiniteDifferenceBinaryBarrierEngine>)
    {
        return make_finite_difference_binary_barrier_engine(settings);
    }
    FiniteDifferenceSettings settings_;
};

/// Validates settings before creating the engine; invalid settings return invalid_parameter.
[[nodiscard]] inline Result<FiniteDifferenceBinaryBarrierEngine> make_finite_difference_binary_barrier_engine(FiniteDifferenceSettings settings)
{
    if (const auto valid = validate_finite_difference_settings(settings); !valid)
        return std::unexpected(valid.error());
    return FiniteDifferenceBinaryBarrierEngine{settings};
}

namespace detail {
template <typename Option>
inline constexpr bool is_builtin_finite_difference_engine<FiniteDifferenceBinaryBarrierEngine, Option> = true;
}

} // namespace kiyosi
