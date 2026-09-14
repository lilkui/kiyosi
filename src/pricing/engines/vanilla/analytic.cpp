#include <kiyosi/pricing/engines/vanilla/analytic.hpp>

#include <algorithm>
#include <cmath>
#include <limits>
#include <numbers>
#include <numeric>

#include "../../detail/black_scholes.hpp"
#include "../../detail/math.hpp"

namespace kiyosi {
using namespace detail;

result<PricingResult> AnalyticVanillaEngine::price_impl(
    const EuropeanOption& option, const PricingContext& context) const
{
    return price_at_volatility(option, context, context.parameters().volatility());
}

result<double> AnalyticVanillaEngine::implied_volatility(
    const EuropeanOption& option, const PricingContext& context, double observed_price,
    ImpliedVolatilitySettings settings) const
{
    if (!std::isfinite(observed_price)) {
        return std::unexpected(Error{error_category::invalid_parameter,
                                     "observed price must be finite"});
    }
    if (!std::isfinite(settings.lower_bound) || !std::isfinite(settings.upper_bound) ||
        settings.lower_bound <= 0.0 || settings.lower_bound >= settings.upper_bound ||
        !std::isfinite(settings.tolerance) || settings.tolerance <= 0.0 ||
        settings.max_iterations <= 0) {
        return std::unexpected(Error{error_category::invalid_parameter,
                                     "implied-volatility settings must be finite, positive, and ordered"});
    }
    const auto valid_expiry = validate_life(context.valuation_time(), option.effective(), option.expiry());
    if (!valid_expiry) {
        return std::unexpected(valid_expiry.error());
    }
    if (context.valuation_date() == option.expiry()) {
        return std::unexpected(Error{error_category::invalid_result,
                                     "implied volatility is undefined at expiry"});
    }

    const double time = actual_365(context.valuation_time(), option.expiry());
    const double spot_discount = std::exp(-context.parameters().dividend_yield() * time);
    const double strike_discount = std::exp(-context.parameters().risk_free_rate() * time);
    const double discounted_spot = context.asset_price() * spot_discount;
    const double discounted_strike = option.strike() * strike_discount;
    const double sign = option.type() == option_type::call ? 1.0 : -1.0;
    const double intrinsic = std::max(sign * (discounted_spot - discounted_strike), 0.0);
    const double upper_price = option.type() == option_type::call ? discounted_spot : discounted_strike;
    const double bound_tolerance = settings.tolerance * std::max({1.0, intrinsic, upper_price});
    if (!std::isfinite(discounted_spot) || !std::isfinite(discounted_strike) ||
        !std::isfinite(intrinsic) || !std::isfinite(upper_price) || !std::isfinite(bound_tolerance)) {
        return std::unexpected(Error{error_category::solver_non_finite,
                                     "implied-volatility bounds are non-finite"});
    }
    if (observed_price < intrinsic - bound_tolerance || observed_price > upper_price + bound_tolerance) {
        return std::unexpected(Error{error_category::invalid_parameter,
                                     "observed price violates the option arbitrage bounds"});
    }

    const auto evaluate = [&](double volatility) -> result<double> {
        const auto priced = price_at_volatility(
            option, context, volatility, risk_measure_output::price_only);
        if (!priced) {
            if (priced.error().category == error_category::invalid_result)
                return std::unexpected(Error{error_category::solver_non_finite,
                                             "implied-volatility pricing became non-finite"});
            return std::unexpected(priced.error());
        }
        const auto value = priced->get(risk_measure::price);
        if (!value || !std::isfinite(*value)) {
            return std::unexpected(Error{error_category::solver_non_finite,
                                         "implied-volatility pricing became non-finite"});
        }
        return *value;
    };

    double lower_bound = settings.lower_bound;
    double upper_bound = settings.upper_bound;
    const auto lower_result = evaluate(lower_bound);
    if (!lower_result) return std::unexpected(lower_result.error());
    const auto upper_result = evaluate(upper_bound);
    if (!upper_result) return std::unexpected(upper_result.error());

    double lower_error = *lower_result - observed_price;
    double upper_error = *upper_result - observed_price;
    if (!std::isfinite(lower_error) || !std::isfinite(upper_error)) {
        return std::unexpected(Error{error_category::solver_non_finite,
                                     "implied-volatility pricing error became non-finite"});
    }
    if (std::abs(lower_error) <= settings.tolerance) return lower_bound;
    if (std::abs(upper_error) <= settings.tolerance) return upper_bound;
    if ((lower_error < 0.0) == (upper_error < 0.0)) {
        return std::unexpected(Error{error_category::unbracketed_volatility,
                                     "observed price is not bracketed by the volatility bounds"});
    }

    double candidate = std::midpoint(lower_bound, upper_bound);
    const double extrinsic = std::max(observed_price - intrinsic, 0.0);
    const double scale = option.type() == option_type::call ? discounted_spot : discounted_strike;
    const double estimate = extrinsic * std::sqrt(2.0 * std::numbers::pi) /
                            (std::max(scale, std::numeric_limits<double>::min()) * std::sqrt(time));
    if (std::isfinite(estimate) && estimate > lower_bound && estimate < upper_bound)
        candidate = estimate;

    for (int iteration = 0; iteration < settings.max_iterations; ++iteration) {
        const auto candidate_result = evaluate(candidate);
        if (!candidate_result) return std::unexpected(candidate_result.error());
        const double candidate_error = *candidate_result - observed_price;
        if (!std::isfinite(candidate_error)) {
            return std::unexpected(Error{error_category::solver_non_finite,
                                         "implied-volatility pricing error became non-finite"});
        }
        if (std::abs(candidate_error) <= settings.tolerance) return candidate;
        if ((lower_bound < upper_bound) &&
            (upper_bound - lower_bound) * 0.5 <= settings.tolerance) {
            return std::midpoint(lower_bound, upper_bound);
        }
        if ((lower_error < 0.0) == (candidate_error < 0.0)) {
            lower_bound = candidate;
            lower_error = candidate_error;
        } else {
            upper_bound = candidate;
            upper_error = candidate_error;
        }

        double next = std::midpoint(lower_bound, upper_bound);
        const double step = std::max({candidate * solver_derivative_step_fraction,
                                      (upper_bound - lower_bound) * solver_bracket_step_fraction,
                                      solver_minimum_derivative_step});
        const double left = std::max(lower_bound, candidate - step);
        const double right = std::min(upper_bound, candidate + step);
        if (right > left) {
            const auto left_result = evaluate(left);
            if (!left_result) return std::unexpected(left_result.error());
            const auto right_result = evaluate(right);
            if (!right_result) return std::unexpected(right_result.error());
            const double left_error = *left_result - observed_price;
            const double right_error = *right_result - observed_price;
            if (!std::isfinite(left_error) || !std::isfinite(right_error)) {
                return std::unexpected(Error{error_category::solver_non_finite,
                                             "implied-volatility pricing error became non-finite"});
            }
            const double first_derivative = (right_error - left_error) / (right - left);
            if (!std::isfinite(first_derivative)) {
                return std::unexpected(Error{error_category::solver_non_finite,
                                             "implied-volatility derivative became non-finite"});
            }
            double second_derivative = 0.0;
            const double left_step = candidate - left;
            const double right_step = right - candidate;
            if (std::abs(left_step - right_step) <=
                    std::numeric_limits<double>::epsilon() * std::max(left_step, right_step) &&
                left_step > 0.0 && right_step > 0.0)
                second_derivative = (right_error - 2.0 * candidate_error + left_error) /
                                    (left_step * left_step);
            if (!std::isfinite(second_derivative)) {
                return std::unexpected(Error{error_category::solver_non_finite,
                                             "implied-volatility curvature became non-finite"});
            }
            const double denominator = 2.0 * first_derivative * first_derivative -
                                       candidate_error * second_derivative;
            const double halley = std::isfinite(denominator) &&
                                          std::abs(denominator) > std::numeric_limits<double>::epsilon()
                                      ? candidate - 2.0 * candidate_error * first_derivative / denominator
                                      : std::numeric_limits<double>::quiet_NaN();
            const double newton = candidate - candidate_error / first_derivative;
            if (std::isfinite(halley) && halley > lower_bound && halley < upper_bound &&
                std::abs(denominator) > std::numeric_limits<double>::epsilon()) {
                next = halley;
            } else if (std::isfinite(newton) && newton > lower_bound && newton < upper_bound) {
                next = newton;
            } else if (std::isfinite(upper_error - lower_error) && upper_error != lower_error) {
                const double secant = lower_bound - lower_error * (upper_bound - lower_bound) /
                                      (upper_error - lower_error);
                if (std::isfinite(secant) && secant > lower_bound && secant < upper_bound)
                    next = secant;
            }
        }
        // Near-zero vega can make secant steps stagnate at a bracket endpoint.
        // Bisect unless the proposed step removes at least 10% of the bracket.
        const double margin = 0.1 * (upper_bound - lower_bound);
        candidate = next > lower_bound + margin && next < upper_bound - margin
                        ? next : std::midpoint(lower_bound, upper_bound);
    }

    return std::unexpected(Error{error_category::solver_non_convergence,
                                 "implied-volatility solver did not converge"});
}

} // namespace kiyosi
