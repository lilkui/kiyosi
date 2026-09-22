#pragma once

#include <utility>

#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/implied.hpp>
#include <kiyosi/pricing/numerical_greeks.hpp>
#include <kiyosi/pricing/result.hpp>
#include <kiyosi/pricing/settings/implied.hpp>
#include <kiyosi/pricing/settings/numerical_shift.hpp>

namespace kiyosi {

/// Wraps any price-only engine so it also reports bump-derived risk and implied quantities
/// without the caller threading shift settings through each call. Infeasible boundary stencils
/// leave their measures unavailable without discarding a valid price; other bump failures remain
/// operation failures.
/// Its thread-safety guarantees are those of the wrapped engine; built-in engines follow the
/// library default.
template <typename Engine>
class NumericalAnalyticsEngine {
public:
    /// Creates an analytics adapter around an engine and numerical-shift settings.
    explicit NumericalAnalyticsEngine(Engine engine = {}, NumericalShiftSettings settings = {})
        : engine_(std::move(engine)), settings_(settings) {}

    /// Prices an option and derives risk measures by revaluation.
    /// @return Pricing measures, or a validation, pricing, or result error.
    template <typename Option>
    [[nodiscard]] Result<PricingResult> price(const Option& option, const PricingContext& context) const
    {
        return calculate_numerical_risk_measures(engine_, option, context, settings_);
    }

    /// Solves for the volatility matching an observed price.
    /// @return Implied volatility, or a validation, bracketing, pricing, or convergence error.
    template <typename Option>
    [[nodiscard]] Result<double> implied_volatility(
        const Option& option, const PricingContext& context, double observed_price,
        ImpliedVolatilitySettings settings = {}) const
    {
        return kiyosi::implied_volatility(engine_, option, context, observed_price, settings);
    }

    /// Solves for an unambiguous product coupon matching an observed price.
    /// @return Implied coupon, or a validation, bracketing, pricing, or convergence error.
    template <typename Option>
    [[nodiscard]] Result<double> implied_coupon(
        const Option& option, const PricingContext& context, double observed_price,
        ImpliedCouponSettings settings = {}) const
        requires requires(const Option& value, double coupon) { detail::replace_coupon(value, coupon); }
    {
        return kiyosi::implied_coupon(engine_, option, context, observed_price, settings);
    }

    /// Solves for a quoted coupon using an explicit maturity-coupon convention.
    /// @return Implied coupon, or a validation, bracketing, pricing, or convergence error.
    template <typename Option>
    [[nodiscard]] Result<double> implied_coupon(
        const Option& option, const PricingContext& context, double observed_price,
        CouponQuoteConvention convention, ImpliedCouponSettings settings = {}) const
        requires requires(const Option& value, double coupon) {
            detail::replace_coupon(value, coupon, CouponQuoteConvention::preserve_maturity_coupon);
        }
    {
        return kiyosi::implied_coupon(engine_, option, context, observed_price, convention, settings);
    }

    /// Returns the wrapped pricing engine.
    const Engine& engine() const noexcept { return engine_; }
    /// Returns the numerical-shift settings.
    NumericalShiftSettings settings() const noexcept { return settings_; }

private:
    Engine engine_;
    NumericalShiftSettings settings_;
};

} // namespace kiyosi
