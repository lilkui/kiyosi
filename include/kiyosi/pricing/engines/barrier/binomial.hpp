#pragma once

#include <kiyosi/instruments/barrier/option.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/detail/binomial.hpp>
#include <kiyosi/pricing/detail/instrument_analytics.hpp>
#include <kiyosi/pricing/result.hpp>
#include <kiyosi/pricing/settings/numerical_shift.hpp>

namespace kiyosi {

class KIYOSI_EXPORT CoxRossRubinsteinBarrierEngine;
[[nodiscard]] Result<CoxRossRubinsteinBarrierEngine> make_cox_ross_rubinstein_barrier_engine(BinomialSettings);

/// Ordinary uniform CRR tree for European single-barrier options.
/// Continuous monitoring checks tree nodes; at-hit rebates pay at the first breached node.
/// Future scheduled observations must coincide exactly with tree steps, otherwise
/// unresolved pricing returns unsupported_operation. Dates and step counts are never adjusted.
/// Delta and gamma are node estimates; gamma needs two steps, and other Greeks are unavailable.
/// Numerical shifts are validated but unused. Prices and Greeks need grid refinement near barriers.
/// Before expiry, volatility must be at least abs(r - q) * sqrt(T / step_count).
/// Concurrent const calls on shared or distinct engines and immutable inputs are safe;
/// concurrent assignment is not. Work buffers are request-local. Allocation failures
/// propagate as std::bad_alloc; calls are synchronous without cancellation.
class KIYOSI_EXPORT CoxRossRubinsteinBarrierEngine {
public:
    CoxRossRubinsteinBarrierEngine() = default;
    [[nodiscard]] Result<double> price(const BarrierOption&, const PricingContext&) const;
    [[nodiscard]] Result<PricingResult> price_with_greeks(const BarrierOption&, const PricingContext&,
                                                          GreeksRequest, NumericalShiftSettings = {}) const;
    [[nodiscard]] BinomialSettings settings() const noexcept { return settings_; }

private:
    explicit CoxRossRubinsteinBarrierEngine(BinomialSettings settings) : settings_(settings) {}
    friend Result<CoxRossRubinsteinBarrierEngine> make_cox_ross_rubinstein_barrier_engine(BinomialSettings);
    BinomialSettings settings_;
};

[[nodiscard]] inline Result<CoxRossRubinsteinBarrierEngine> make_cox_ross_rubinstein_barrier_engine(BinomialSettings settings)
{
    const auto valid = validate_binomial_settings(settings);
    if (!valid) return std::unexpected(valid.error());
    return CoxRossRubinsteinBarrierEngine{settings};
}

namespace detail {
template <typename Option>
Result<ImpliedVolatilitySettings> prepare_implied_volatility(
    const CoxRossRubinsteinBarrierEngine& engine, const Option& option, const PricingContext& context,
    ImpliedVolatilitySettings settings)
{
    return prepare_binomial_implied_volatility(engine.settings(), option, context, settings);
}
} // namespace detail
} // namespace kiyosi
