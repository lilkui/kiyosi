#pragma once

#include <algorithm>
#include <array>
#include <cmath>
#include <limits>
#include <numeric>
#include <ranges>

#include <ito/pricing/result.hpp>
#include <ito/market/context.hpp>
#include <ito/instruments/vanilla.hpp>

namespace ito::detail {

constexpr double percentage_point = 100.0;
constexpr double inverse_sqrt_two = 0.70710678118654752440;
constexpr double inverse_sqrt_two_pi = 0.39894228040143267794;
constexpr double solver_derivative_step_fraction = 1e-3;
constexpr double solver_bracket_step_fraction = 1e-2;
constexpr double solver_minimum_derivative_step = 1e-6;

inline double actual_365(date start, date end) noexcept
{
    return *year_fraction(start, end);
}

inline double normal_cdf(double value) noexcept
{
    return 0.5 * std::erfc(-value * inverse_sqrt_two);
}

inline double normal_pdf(double value) noexcept
{
    return inverse_sqrt_two_pi * std::exp(-0.5 * value * value);
}

inline PricingResult price_only_result(double value)
{
    const double unavailable = std::numeric_limits<double>::quiet_NaN();
    return PricingResult{value, unavailable, unavailable, unavailable, unavailable, unavailable,
                         unavailable, unavailable, unavailable, unavailable, unavailable,
                         risk_bit(risk_measure::price)};
}

inline result<PricingResult> price_at_volatility(
    const EuropeanOption& option, const PricingContext& context, double volatility,
    PricingRequest request = PricingRequest::all())
{
    const auto valid_expiry = validate_expiry(context.valuation_date(), option.expiry());
    if (!valid_expiry) {
        return std::unexpected(valid_expiry.error());
    }

    const double spot = context.asset_price().value();
    const double strike = option.strike();
    const double sign = option.type() == option_type::call ? 1.0 : -1.0;
    const double year_fraction = actual_365(context.valuation_date(), option.expiry());

    if (year_fraction == 0.0) {
        const double value = std::max(sign * (spot - strike), 0.0);
        if (request.measures == risk_bit(risk_measure::price)) return price_only_result(value);
        auto output = PricingResult{value, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0};
        output.available = risk_bit(risk_measure::price);
        return output;
    }

    const double rate = context.parameters().risk_free_rate();
    const double dividend = context.parameters().dividend_yield();
    const double sqrt_time = std::sqrt(year_fraction);
    const double volatility_time = volatility * sqrt_time;
    if (!std::isfinite(volatility_time)) {
        return std::unexpected(Error{error_category::invalid_result,
                                     "analytic pricing produced an unstable volatility limit"});
    }
    if (volatility_time < 1e-10) {
        const double forward = spot * std::exp((rate - dividend) * year_fraction);
        const double discount = std::exp(-rate * year_fraction);
        const double intrinsic = sign * (forward - strike);
        const double value = discount * std::max(intrinsic, 0.0);
        if (!std::isfinite(value))
            return std::unexpected(Error{error_category::invalid_result,
                                         "analytic pricing produced a non-finite result"});
        if (request.measures == risk_bit(risk_measure::price)) return price_only_result(value);
        const double delta = intrinsic > 0.0 ? sign * std::exp(-dividend * year_fraction) : 0.0;
        auto output = PricingResult{value, delta, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0};
        output.available = risk_bit(risk_measure::price) | risk_bit(risk_measure::delta);
        return output;
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
        return std::unexpected(Error{error_category::invalid_result,
                                     "analytic pricing produced a non-finite result"});
    if (request.measures == risk_bit(risk_measure::price)) return price_only_result(value);
    const double delta = sign * dividend_discount_factor * cumulative_d1;
    const double density_d1 = normal_pdf(d1);
    const double carry = sign * dividend * spot * dividend_discount_factor * cumulative_d1 -
                         sign * rate * strike * rate_discount_factor * cumulative_d2;
    double gamma = 0.0;
    double speed = 0.0;
    double theta = 0.0;
    double charm = 0.0;
    double color = 0.0;
    double vega = 0.0;
    double vanna = 0.0;
    double zomma = 0.0;
    if (density_d1 != 0.0 && std::isfinite(d1) && std::isfinite(d2)) {
        gamma = dividend_discount_factor * density_d1 / (spot * volatility * sqrt_time);
        speed = -gamma * (1.0 + d1 / (volatility * sqrt_time)) / spot;
        theta = (-spot * dividend_discount_factor * density_d1 * volatility /
                     (2.0 * sqrt_time) + carry) /
                365.0;
        charm = -dividend_discount_factor *
                (density_d1 * ((rate - dividend) / (volatility * sqrt_time) -
                               0.5 * d2 / year_fraction) -
                 sign * dividend * cumulative_d1) /
                365.0;
        color = gamma *
                (dividend + (rate - dividend) * d1 / (volatility * sqrt_time) +
                 (1.0 - d1 * d2) / (2.0 * year_fraction)) /
                365.0;
        vega = spot * dividend_discount_factor * density_d1 * sqrt_time / percentage_point;
        vanna = -dividend_discount_factor * d2 * density_d1 /
                (volatility * percentage_point);
        zomma = gamma * (d1 * d2 - 1.0) /
                (volatility * percentage_point);
    } else {
        theta = carry / 365.0;
    }
    const double rho = sign * year_fraction * strike * rate_discount_factor * cumulative_d2 /
                       percentage_point;
    const PricingResult output{value, delta, gamma, speed, theta, charm,
                               color, vega, vanna, zomma, rho};
    const std::array values{value, delta, gamma, speed, theta, charm,
                            color, vega, vanna, zomma, rho};
    if (!std::ranges::all_of(values, [](double item) { return std::isfinite(item); })) {
        return std::unexpected(Error{error_category::invalid_result,
                                     "analytic pricing produced a non-finite result"});
    }
    return output;
}

inline result<PricingResult> select_outputs(
    result<PricingResult> priced, PricingRequest request, risk_measure_set supported)
{
    if (!priced) return priced;
    if (request.measures == all_risk_measures) request.measures = supported;
    if ((request.measures & ~supported) != 0)
        return std::unexpected(Error{error_category::unsupported_risk_measure,
                                     "requested risk measure is unsupported by this engine"});
    priced->available &= request.measures & supported;
    const double unavailable = std::numeric_limits<double>::quiet_NaN();
    const auto clear = [&](risk_measure measure, double& field) {
        if (!request.requests(measure) || !priced->has(measure)) field = unavailable;
    };
    clear(risk_measure::price, priced->value);
    clear(risk_measure::delta, priced->delta);
    clear(risk_measure::gamma, priced->gamma);
    clear(risk_measure::speed, priced->speed);
    clear(risk_measure::theta, priced->theta);
    clear(risk_measure::charm, priced->charm);
    clear(risk_measure::color, priced->color);
    clear(risk_measure::vega, priced->vega);
    clear(risk_measure::vanna, priced->vanna);
    clear(risk_measure::zomma, priced->zomma);
    clear(risk_measure::rho, priced->rho);
    return priced;
}

} // namespace ito::detail
