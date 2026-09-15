#pragma once

#include <span>
#include <utility>

#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/implied.hpp>
#include <kiyosi/pricing/numerical_greeks.hpp>
#include <kiyosi/pricing/result.hpp>
#include <kiyosi/pricing/scenario.hpp>
#include <kiyosi/pricing/settings/implied.hpp>
#include <kiyosi/pricing/settings/numerical_shift.hpp>

namespace kiyosi {

/// Wraps any price-only engine so it also reports bump-derived risk, scenario grids, and
/// implied quantities without the caller threading shift settings through each call.
/// Its thread-safety guarantees are those of the wrapped engine; built-in engines follow the
/// library default.
template <typename Engine>
class NumericalAnalyticsEngine {
public:
    explicit NumericalAnalyticsEngine(Engine engine = {}, NumericalShiftSettings settings = {})
        : engine_(std::move(engine)), settings_(settings) {}

    template <typename Option>
    [[nodiscard]] result<PricingResult> price(const Option& option, const PricingContext& context) const
    {
        return numerical_analytics(engine_, option, context, settings_);
    }

    template <typename Option>
    [[nodiscard]] result<ScenarioGridResult> scenario_grid(
        const Option& option, const PricingContext& context, std::span<const double> spots) const
    {
        return kiyosi::scenario_grid(engine_, option, context, spots, settings_);
    }

    template <typename Option>
    [[nodiscard]] result<double> implied_volatility(
        const Option& option, const PricingContext& context, double observed_price,
        ImpliedVolatilitySettings settings = {}) const
    {
        return kiyosi::implied_volatility(engine_, option, context, observed_price, settings);
    }

    template <typename Option>
    [[nodiscard]] result<double> implied_coupon(
        const Option& option, const PricingContext& context, double observed_price,
        ImpliedCouponSettings settings = {}) const
    requires requires(const Option& value, double coupon) { value.with_coupon_rate(coupon); }
    {
        return kiyosi::implied_coupon(engine_, option, context, observed_price, settings);
    }

    const Engine& engine() const noexcept { return engine_; }
    NumericalShiftSettings settings() const noexcept { return settings_; }

private:
    Engine engine_;
    NumericalShiftSettings settings_;
};

} // namespace kiyosi
