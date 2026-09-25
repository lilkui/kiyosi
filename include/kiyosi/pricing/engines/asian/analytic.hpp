#pragma once

#include <kiyosi/instruments/asian.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/numerical_greeks.hpp>

namespace kiyosi {

/// Closed-form continuous geometric average-price option under lognormal spot.
class KIYOSI_EXPORT AnalyticGeometricAveragePriceEngine {
public:
    /// Prices a geometric-average option. Once averaging has begun, its realized average must be positive.
    /// Before averaging begins, the realized average must be zero.
    /// @return Price, or a contract or context error.
    [[nodiscard]] Result<double> price(const GeometricAveragePriceOption& option, const PricingContext& context) const
    {
        return detail::price_value(price_native(option, context));
    }

    /// Prices with the explicitly requested Greeks; unavailable measures remain empty.
    [[nodiscard]] Result<PricingResult> price_with_greeks(const GeometricAveragePriceOption& option, const PricingContext& context,
        GreeksLevel level, NumericalShiftSettings settings = {}) const
    {
        return detail::price_with_greeks(*this, option, context, level, settings,
            [&](const auto& engine) {
                return engine.price_native(option, context);
            });
    }


private:
    [[nodiscard]] Result<PricingResult> price_native(const GeometricAveragePriceOption& option, const PricingContext& context) const;
};

/// Turnbull-Wakeman moment-matched approximation for arithmetic averaging.
class KIYOSI_EXPORT TurnbullWakemanArithmeticAveragePriceEngine {
public:
    /// Prices an arithmetic-average option using moment matching. For a nonzero averaging window,
    /// the realized average must be zero before averaging begins and positive afterward. A zero-length
    /// window is a single fixing at expiry and follows European vanilla pricing before expiry.
    /// @return Price, or a contract or context error.
    [[nodiscard]] Result<double> price(const ArithmeticAveragePriceOption& option, const PricingContext& context) const
    {
        return detail::price_value(price_native(option, context));
    }

    /// Prices with the explicitly requested Greeks; unavailable measures remain empty.
    [[nodiscard]] Result<PricingResult> price_with_greeks(const ArithmeticAveragePriceOption& option, const PricingContext& context,
        GreeksLevel level, NumericalShiftSettings settings = {}) const
    {
        return detail::price_with_greeks(*this, option, context, level, settings,
            [&](const auto& engine) {
                return engine.price_native(option, context);
            });
    }


private:
    [[nodiscard]] Result<PricingResult> price_native(const ArithmeticAveragePriceOption& option, const PricingContext& context) const;
};

} // namespace kiyosi
