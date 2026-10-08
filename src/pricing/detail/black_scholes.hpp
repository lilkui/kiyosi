#pragma once

#include <algorithm>
#include <cmath>
#include <numbers>

#include <kiyosi/instruments/vanilla.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>

#include "math.hpp"

namespace kiyosi::detail {

inline constexpr double minimum_black_scholes_volatility_time = 1e-10;

struct BlackScholesProbabilities {
    double d1;
    double d2;
    double asset;
    double cash;
};

inline BlackScholesProbabilities black_scholes_probabilities(
    double sign, double spot, double strike, double rate, double dividend, double volatility, double time)
{
    const double volatility_time = volatility * std::sqrt(time);
    if (volatility_time < minimum_black_scholes_volatility_time) {
        const double exercised = sign * (spot * std::exp((rate - dividend) * time) - strike) > 0.0 ? 1.0 : 0.0;
        return {0.0, 0.0, exercised, exercised};
    }
    const double standardized_forward =
        (std::log(spot) - std::log(strike) + (rate - dividend) * time) / volatility_time;
    const double d1 = standardized_forward + 0.5 * volatility_time;
    const double d2 = standardized_forward - 0.5 * volatility_time;
    return {d1, d2, normal_cdf(sign * d1), normal_cdf(sign * d2)};
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
        return make_pricing_result(value);
    }

    const double rate = context.model_parameters().risk_free_rate();
    const double dividend = context.model_parameters().dividend_yield();
    const double sqrt_time = std::sqrt(year_fraction);
    const double volatility_time = volatility * sqrt_time;
    if (!std::isfinite(volatility_time)) {
        return std::unexpected(Error{ErrorCategory::invalid_result,
                                     "analytic pricing produced an unstable volatility limit"});
    }
    if (volatility_time < minimum_black_scholes_volatility_time) {
        const double forward = spot * std::exp((rate - dividend) * year_fraction);
        const double discount = std::exp(-rate * year_fraction);
        const double intrinsic = sign * (forward - strike);
        const double value = discount * std::max(intrinsic, 0.0);
        if (!std::isfinite(value))
            return std::unexpected(Error{ErrorCategory::invalid_result,
                                         "analytic pricing produced a non-finite result"});
        if (requested_output.empty()) return make_pricing_result(value);
        const double delta = intrinsic > 0.0 ? sign * std::exp(-dividend * year_fraction) : 0.0;
        return make_pricing_result(value, {{Greek::delta, requested_output.has(Greek::delta) ? std::optional{delta} : std::nullopt}});
    }

    const auto probabilities = black_scholes_probabilities(sign, spot, strike, rate, dividend, volatility, year_fraction);
    const double d1 = probabilities.d1;
    const double d2 = probabilities.d2;
    const double dividend_discount_factor = std::exp(-dividend * year_fraction);
    const double rate_discount_factor = std::exp(-rate * year_fraction);
    const double cumulative_d1 = probabilities.asset;
    const double cumulative_d2 = probabilities.cash;

    const double asset_value = sign * d1 < -10.0
                                   ? exponential_normal_cdf(std::log(spot) - dividend * year_fraction, sign * d1)
                                   : spot * dividend_discount_factor * cumulative_d1;
    const double cash_value = sign * d2 < -10.0
                                  ? exponential_normal_cdf(std::log(strike) - rate * year_fraction, sign * d2)
                                  : strike * rate_discount_factor * cumulative_d2;
    const double value = sign * (asset_value - cash_value);
    if (!std::isfinite(value))
        return std::unexpected(Error{ErrorCategory::invalid_result,
                                     "analytic pricing produced a non-finite result"});
    if (requested_output.empty()) return make_pricing_result(value);

    const auto want = [&](Greek greek) { return requested_output.has(greek); };
    const double log_density = -0.5 * d1 * d1 + std::log(inverse_sqrt_two_pi);
    const bool regular = std::isfinite(log_density) && std::isfinite(d2) && std::isfinite(d1 * d2);
    // Apply all scale factors before exponentiating so representable derivatives survive tail underflow.
    const auto weighted_density = [&](double log_weight, double factor = 1.0) {
        if (!regular || factor == 0.0) return 0.0;
        return std::copysign(std::exp(log_density + log_weight + std::log(std::abs(factor))), factor);
    };
    const double log_spot = std::log(spot);
    const double log_volatility = std::log(volatility);
    const double log_root_time = std::log(sqrt_time);
    const double log_discount = -dividend * year_fraction;
    const double log_gamma_weight = log_discount - log_spot - log_volatility - log_root_time;
    const bool needs_gamma = want(Greek::gamma) || want(Greek::speed) ||
                             want(Greek::color) || want(Greek::zomma);
    const double gamma = needs_gamma
                             ? weighted_density(log_gamma_weight)
                             : 0.0;
    std::optional<double> speed, theta, charm, color, vega, vanna, zomma;
    const double carry = want(Greek::theta)
                             ? sign * dividend * asset_value - sign * rate * cash_value
                             : 0.0;
    if (regular) {
        if (want(Greek::speed)) speed = -weighted_density(log_gamma_weight - log_spot, 1.0 + d1 / volatility_time);
        if (want(Greek::theta)) theta = (-weighted_density(log_spot + log_discount + log_volatility - std::numbers::ln2 - log_root_time) + carry) / 365.0;
        if (want(Greek::charm)) charm = (-weighted_density(log_discount, (rate - dividend) / volatility_time - 0.5 * d2 / year_fraction) +
                                         sign * dividend * dividend_discount_factor * cumulative_d1) /
                                        365.0;
        if (want(Greek::color)) color =
                                    (weighted_density(log_gamma_weight, dividend) +
                                     std::copysign(1.0, d1) * weighted_density(log_gamma_weight - log_volatility - log_root_time + std::log(std::abs(d1)), rate - dividend) +
                                     weighted_density(log_gamma_weight - std::numbers::ln2 - std::log(year_fraction), 1.0 - d1 * d2)) /
                                    365.0;
        if (want(Greek::vega)) vega = weighted_density(log_spot + log_discount + log_root_time - std::log(percentage_points_per_unit));
        if (want(Greek::vanna)) vanna = -weighted_density(log_discount - log_volatility - std::log(percentage_points_per_unit), d2);
        if (want(Greek::zomma)) zomma = weighted_density(log_gamma_weight - log_volatility - std::log(percentage_points_per_unit), d1 * d2 - 1.0);
    } else {
        if (want(Greek::speed)) speed = 0.0;
        if (want(Greek::theta)) theta = carry / 365.0;
        if (want(Greek::charm)) charm = sign * dividend * dividend_discount_factor * cumulative_d1 / 365.0;
        if (want(Greek::color)) color = 0.0;
        if (want(Greek::vega)) vega = 0.0;
        if (want(Greek::vanna)) vanna = 0.0;
        if (want(Greek::zomma)) zomma = 0.0;
    }
    const std::optional<double> rho = want(Greek::rho)
                                          ? std::optional{sign * year_fraction * cash_value / percentage_points_per_unit}
                                          : std::nullopt;
    auto output = make_pricing_result(value, {{Greek::delta, want(Greek::delta) ? std::optional{sign * dividend_discount_factor * cumulative_d1} : std::nullopt}, {Greek::gamma, want(Greek::gamma) ? std::optional{gamma} : std::nullopt}, {Greek::speed, speed}, {Greek::theta, theta}, {Greek::charm, charm}, {Greek::color, color}, {Greek::vega, vega}, {Greek::vanna, vanna}, {Greek::zomma, zomma}, {Greek::rho, rho}});
    if (!output) return std::unexpected(output.error());
    if (!output->all_finite()) {
        return std::unexpected(Error{ErrorCategory::invalid_result,
                                     "analytic pricing produced a non-finite result"});
    }
    return output;
}

} // namespace kiyosi::detail
