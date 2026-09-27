#pragma once

#include <algorithm>
#include <cmath>

#include <kiyosi/instruments/vanilla.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>

#include "math.hpp"

namespace kiyosi::detail {

inline Result<PricingResult> price_only_result(double value)
{
    return make_pricing_result({{RiskMeasure::price, value}});
}

/// Black-Scholes-Merton valuation of a European vanilla with selected analytic Greeks.
/// The volatility is supplied separately so solvers can reprice without rebuilding the context.
inline Result<PricingResult> price_at_volatility(
    const EuropeanOption& option, const PricingContext& context, double volatility,
    GreeksRequest requested_output = true)
{
    const auto valid_expiry = validate_valuation_within_instrument_life(context.valuation_time(), option.effective_date(), option.expiry_date());
    if (!valid_expiry) return std::unexpected(valid_expiry.error());

    const double spot = context.spot_price();
    const double strike = option.strike();
    const double sign = option.option_type() == OptionType::call ? 1.0 : -1.0;
    const double year_fraction = actual_365_fixed_year_fraction(context.valuation_time(), option.expiry_date());

    if (year_fraction == 0.0) {
        const double value = std::max(sign * (spot - strike), 0.0);
        if (requested_output.empty()) return price_only_result(value);
        return make_pricing_result({{RiskMeasure::price, value}});
    }

    const double rate = context.model_parameters().risk_free_rate();
    const double dividend = context.model_parameters().dividend_yield();
    const double sqrt_time = std::sqrt(year_fraction);
    const double volatility_time = volatility * sqrt_time;
    if (!std::isfinite(volatility_time)) {
        return std::unexpected(Error{ErrorCategory::invalid_result,
                                     "analytic pricing produced an unstable volatility limit"});
    }
    if (volatility_time < 1e-10) {
        const double forward = spot * std::exp((rate - dividend) * year_fraction);
        const double discount = std::exp(-rate * year_fraction);
        const double intrinsic = sign * (forward - strike);
        const double value = discount * std::max(intrinsic, 0.0);
        if (!std::isfinite(value))
            return std::unexpected(Error{ErrorCategory::invalid_result,
                                         "analytic pricing produced a non-finite result"});
        if (requested_output.empty()) return price_only_result(value);
        const double delta = intrinsic > 0.0 ? sign * std::exp(-dividend * year_fraction) : 0.0;
        return make_pricing_result(
            {{RiskMeasure::price, value}, {RiskMeasure::delta, requested_output.has(RiskMeasure::delta) ? std::optional{delta} : std::nullopt}});
    }

    const double d1 = (std::log(spot / strike) +
                       (rate - dividend + 0.5 * volatility * volatility) * year_fraction) /
                      (volatility * sqrt_time);
    const double d2 = d1 - volatility * sqrt_time;
    const double dividend_discount_factor = std::exp(-dividend * year_fraction);
    const double rate_discount_factor = std::exp(-rate * year_fraction);
    const double cumulative_d1 = normal_cdf(sign * d1);
    const double cumulative_d2 = normal_cdf(sign * d2);

    const double value = sign * (spot * dividend_discount_factor * cumulative_d1 -
                                 strike * rate_discount_factor * cumulative_d2);
    if (!std::isfinite(value))
        return std::unexpected(Error{ErrorCategory::invalid_result,
                                     "analytic pricing produced a non-finite result"});
    if (requested_output.empty()) return price_only_result(value);

    const auto want = [&](RiskMeasure greek) { return requested_output.has(greek); };
    const double density_d1 = normal_pdf(d1);
    const bool regular = density_d1 != 0.0 && std::isfinite(d1) && std::isfinite(d2);
    const bool needs_gamma = want(RiskMeasure::gamma) || want(RiskMeasure::speed) ||
                             want(RiskMeasure::color) || want(RiskMeasure::zomma);
    const double gamma = needs_gamma && regular
                             ? dividend_discount_factor * density_d1 / (spot * volatility * sqrt_time)
                             : 0.0;
    std::optional<double> speed, theta, charm, color, vega, vanna, zomma;
    const double carry = want(RiskMeasure::theta)
                             ? sign * dividend * spot * dividend_discount_factor * cumulative_d1 -
                                   sign * rate * strike * rate_discount_factor * cumulative_d2
                             : 0.0;
    if (density_d1 != 0.0 && std::isfinite(d1) && std::isfinite(d2)) {
        if (want(RiskMeasure::speed)) speed = -gamma * (1.0 + d1 / (volatility * sqrt_time)) / spot;
        if (want(RiskMeasure::theta)) theta = (-spot * dividend_discount_factor * density_d1 * volatility / (2.0 * sqrt_time) + carry) / 365.0;
        if (want(RiskMeasure::charm)) charm = -dividend_discount_factor *
                                              (density_d1 * ((rate - dividend) / (volatility * sqrt_time) - 0.5 * d2 / year_fraction) -
                                               sign * dividend * cumulative_d1) /
                                              365.0;
        if (want(RiskMeasure::color)) color = gamma *
                                              (dividend + (rate - dividend) * d1 / (volatility * sqrt_time) +
                                               (1.0 - d1 * d2) / (2.0 * year_fraction)) /
                                              365.0;
        if (want(RiskMeasure::vega)) vega = spot * dividend_discount_factor * density_d1 * sqrt_time / percentage_points_per_unit;
        if (want(RiskMeasure::vanna)) vanna = -dividend_discount_factor * d2 * density_d1 / (volatility * percentage_points_per_unit);
        if (want(RiskMeasure::zomma)) zomma = gamma * (d1 * d2 - 1.0) / (volatility * percentage_points_per_unit);
    } else {
        if (want(RiskMeasure::speed)) speed = 0.0;
        if (want(RiskMeasure::theta)) theta = carry / 365.0;
        if (want(RiskMeasure::charm)) charm = 0.0;
        if (want(RiskMeasure::color)) color = 0.0;
        if (want(RiskMeasure::vega)) vega = 0.0;
        if (want(RiskMeasure::vanna)) vanna = 0.0;
        if (want(RiskMeasure::zomma)) zomma = 0.0;
    }
    const std::optional<double> rho = want(RiskMeasure::rho)
                                          ? std::optional{sign * year_fraction * strike * rate_discount_factor * cumulative_d2 / percentage_points_per_unit}
                                          : std::nullopt;
    auto output = make_pricing_result(
        {{RiskMeasure::price, value}, {RiskMeasure::delta, want(RiskMeasure::delta) ? std::optional{sign * dividend_discount_factor * cumulative_d1} : std::nullopt}, {RiskMeasure::gamma, want(RiskMeasure::gamma) ? std::optional{gamma} : std::nullopt}, {RiskMeasure::speed, speed}, {RiskMeasure::theta, theta}, {RiskMeasure::charm, charm}, {RiskMeasure::color, color}, {RiskMeasure::vega, vega}, {RiskMeasure::vanna, vanna}, {RiskMeasure::zomma, zomma}, {RiskMeasure::rho, rho}});
    if (!output) return std::unexpected(output.error());
    if (!output->all_finite()) {
        return std::unexpected(Error{ErrorCategory::invalid_result,
                                     "analytic pricing produced a non-finite result"});
    }
    return output;
}

} // namespace kiyosi::detail
