#include <ito/ito.hpp>

#include <algorithm>
#include <array>
#include <cmath>
#include <numeric>
#include <ranges>

namespace ito {
namespace {

constexpr double days_per_year = 365.0;
constexpr double percentage_point = 100.0;
constexpr double inverse_sqrt_two = 0.70710678118654752440;
constexpr double inverse_sqrt_two_pi = 0.39894228040143267794;

double normal_cdf(double value) noexcept
{
    return 0.5 * std::erfc(-value * inverse_sqrt_two);
}

double normal_pdf(double value) noexcept
{
    return inverse_sqrt_two_pi * std::exp(-0.5 * value * value);
}

result<PricingResult> price_at_volatility(
    const EuropeanOption& option, const PricingContext& context, double volatility)
{
    const auto valid_expiry = validate_expiry(context.valuation_date(), option.expiry());
    if (!valid_expiry) {
        return std::unexpected(valid_expiry.error());
    }

    const double spot = context.asset_price().value();
    const double strike = option.strike();
    const double sign = option.type() == option_type::call ? 1.0 : -1.0;
    const double year_fraction = static_cast<double>(
        (option.expiry() - context.valuation_date()).count()) / days_per_year;

    if (year_fraction == 0.0) {
        return PricingResult{std::max(sign * (spot - strike), 0.0),
                             0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0};
    }

    const double rate = context.parameters().risk_free_rate();
    const double dividend = context.parameters().dividend_yield();
    const double sqrt_time = std::sqrt(year_fraction);
    const double d1 = (std::log(spot / strike) +
                       (rate - dividend + 0.5 * volatility * volatility) * year_fraction) /
                      (volatility * sqrt_time);
    const double d2 = d1 - volatility * sqrt_time;
    const double dividend_discount_factor = std::exp(-dividend * year_fraction);
    const double rate_discount_factor = std::exp(-rate * year_fraction);
    const double density_d1 = normal_pdf(d1);
    const double cumulative_d1 = normal_cdf(sign * d1);
    const double cumulative_d2 = normal_cdf(sign * d2);

    const double value = sign * (spot * dividend_discount_factor * cumulative_d1 -
                                 strike * rate_discount_factor * cumulative_d2);
    const double delta = sign * dividend_discount_factor * cumulative_d1;
    const double gamma = dividend_discount_factor * density_d1 / (spot * volatility * sqrt_time);
    const double speed = -gamma * (1.0 + d1 / (volatility * sqrt_time)) / spot;
    const double theta = (-spot * dividend_discount_factor * density_d1 * volatility /
                              (2.0 * sqrt_time) +
                          sign * dividend * spot * dividend_discount_factor * cumulative_d1 -
                          sign * rate * strike * rate_discount_factor * cumulative_d2) /
                         days_per_year;
    const double charm = -dividend_discount_factor *
                         (density_d1 * ((rate - dividend) / (volatility * sqrt_time) -
                                        0.5 * d2 / year_fraction) -
                          sign * dividend * cumulative_d1) /
                         days_per_year;
    const double color = gamma *
                         (dividend + (rate - dividend) * d1 / (volatility * sqrt_time) +
                          (1.0 - d1 * d2) / (2.0 * year_fraction)) /
                         days_per_year;
    const double vega = spot * dividend_discount_factor * density_d1 * sqrt_time / percentage_point;
    const double vanna = -dividend_discount_factor * d2 * density_d1 /
                         (volatility * percentage_point);
    const double zomma = gamma * (d1 * d2 - 1.0) /
                         (volatility * percentage_point);
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

}

result<PricingResult> AnalyticEuropeanEngine::price(
    const EuropeanOption& option, const PricingContext& context) const
{
    return price_at_volatility(option, context, context.parameters().volatility());
}

result<double> AnalyticEuropeanEngine::implied_volatility(
    const EuropeanOption& option, const PricingContext& context, double observed_price,
    ImpliedVolatilitySettings settings) const
{
    if (!std::isfinite(observed_price) || observed_price < 0.0) {
        return std::unexpected(Error{error_category::invalid_parameter,
                                     "observed price must be finite and non-negative"});
    }
    if (!std::isfinite(settings.lower_bound) || !std::isfinite(settings.upper_bound) ||
        settings.lower_bound <= 0.0 || settings.lower_bound >= settings.upper_bound ||
        !std::isfinite(settings.tolerance) || settings.tolerance <= 0.0 ||
        settings.max_iterations <= 0) {
        return std::unexpected(Error{error_category::invalid_parameter,
                                     "implied-volatility settings must be finite, positive, and ordered"});
    }
    const auto valid_expiry = validate_expiry(context.valuation_date(), option.expiry());
    if (!valid_expiry) {
        return std::unexpected(valid_expiry.error());
    }
    if (context.valuation_date() == option.expiry()) {
        return std::unexpected(Error{error_category::invalid_result,
                                     "implied volatility is undefined at expiry"});
    }

    double lower_bound = settings.lower_bound;
    double upper_bound = settings.upper_bound;
    const auto lower_result = price_at_volatility(option, context, lower_bound);
    if (!lower_result) return std::unexpected(lower_result.error());
    const auto upper_result = price_at_volatility(option, context, upper_bound);
    if (!upper_result) return std::unexpected(upper_result.error());

    double lower_error = lower_result->value - observed_price;
    const double upper_error = upper_result->value - observed_price;
    if (std::abs(lower_error) <= settings.tolerance) return lower_bound;
    if (std::abs(upper_error) <= settings.tolerance) return upper_bound;
    if ((lower_error < 0.0) == (upper_error < 0.0)) {
        return std::unexpected(Error{error_category::invalid_result,
                                     "observed price is not bracketed by the volatility bounds"});
    }

    for (int iteration = 0; iteration < settings.max_iterations; ++iteration) {
        const double midpoint = std::midpoint(lower_bound, upper_bound);
        const auto midpoint_result = price_at_volatility(option, context, midpoint);
        if (!midpoint_result) return std::unexpected(midpoint_result.error());
        const double midpoint_error = midpoint_result->value - observed_price;
        if (std::abs(midpoint_error) <= settings.tolerance ||
            (upper_bound - lower_bound) * 0.5 <= settings.tolerance) {
            return midpoint;
        }
        if ((lower_error < 0.0) == (midpoint_error < 0.0)) {
            lower_bound = midpoint;
            lower_error = midpoint_error;
        } else {
            upper_bound = midpoint;
        }
    }

    return std::unexpected(Error{error_category::invalid_result,
                                 "implied-volatility solver did not converge"});
}

}
