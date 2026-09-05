#include <ito/ito.hpp>

#include <algorithm>
#include <array>
#include <cmath>
#include <functional>
#include <numeric>
#include <numbers>
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

result<PricingResult> price_binomial_american(
    const EuropeanOption& option, const PricingContext& context, BinomialAmericanSettings settings)
{
    // ponytail: O(N²) rollback with O(N) memory; optimize to a recombining index kernel if profiling requires it.
    const auto valid_expiry = validate_expiry(context.valuation_date(), option.expiry());
    if (!valid_expiry) return std::unexpected(valid_expiry.error());
    if (settings.steps <= 0 || settings.steps > 1'000'000) {
        return std::unexpected(Error{error_category::invalid_parameter,
                                     "binomial step count must be between 1 and 1000000"});
    }

    const double spot = context.asset_price().value();
    const double strike = option.strike();
    const double sign = option.type() == option_type::call ? 1.0 : -1.0;
    const double time = static_cast<double>(
        (option.expiry() - context.valuation_date()).count()) / days_per_year;
    if (time == 0.0) {
        return PricingResult{std::max(sign * (spot - strike), 0.0),
                             0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0};
    }

    const double rate = context.parameters().risk_free_rate();
    const double dividend = context.parameters().dividend_yield();
    const double volatility = context.parameters().volatility();
    const double dt = time / static_cast<double>(settings.steps);
    const double root_dt = std::sqrt(dt);
    const double up = std::exp(volatility * root_dt);
    const double down = 1.0 / up;
    const double growth = std::exp((rate - dividend) * dt);
    const double discount = std::exp(-rate * dt);
    const double probability = (growth - down) / (up - down);
    if (!std::isfinite(up) || !std::isfinite(down) || !std::isfinite(discount) ||
        !std::isfinite(probability) || probability < 0.0 || probability > 1.0) {
        return std::unexpected(Error{error_category::invalid_result,
                                     "binomial parameters produced an unstable tree"});
    }

    std::vector<double> values(static_cast<std::size_t>(settings.steps) + 1);
    for (int node = 0; node <= settings.steps; ++node) {
        const double node_spot = spot * std::pow(up, node) *
                                 std::pow(down, settings.steps - node);
        if (!std::isfinite(node_spot)) {
            return std::unexpected(Error{error_category::invalid_result,
                                         "binomial tree produced a non-finite asset price"});
        }
        values[static_cast<std::size_t>(node)] = std::max(sign * (node_spot - strike), 0.0);
    }

    std::array<double, 3> level_two{};
    std::array<double, 2> level_one{};
    if (settings.steps == 1) level_one = {values[0], values[1]};
    if (settings.steps == 2) level_two = {values[0], values[1], values[2]};
    for (int level = settings.steps - 1; level >= 0; --level) {
        for (int node = 0; node <= level; ++node) {
            const std::size_t index = static_cast<std::size_t>(node);
            const double continuation = discount *
                (probability * values[index + 1] + (1.0 - probability) * values[index]);
            const double node_spot = spot * std::pow(up, node) * std::pow(down, level - node);
            values[index] = std::max(continuation, sign * (node_spot - strike));
            if (!std::isfinite(values[index])) {
                return std::unexpected(Error{error_category::invalid_result,
                                             "binomial pricing produced a non-finite result"});
            }
        }
        if (level == 2) {
            level_two = {values[0], values[1], values[2]};
        } else if (level == 1) {
            level_one = {values[0], values[1]};
        }
    }

    double delta = 0.0;
    double gamma = 0.0;
    if (settings.steps >= 1) {
        const double denominator = spot * (up - down);
        if (std::isfinite(denominator) && denominator != 0.0)
            delta = (level_one[1] - level_one[0]) / denominator;
    }
    if (settings.steps >= 2) {
        const double delta_up_denominator = spot * (up * up - 1.0);
        const double delta_down_denominator = spot * (1.0 - down * down);
        const double gamma_denominator = 0.5 * spot * (up * up - down * down);
        if (delta_up_denominator != 0.0 && delta_down_denominator != 0.0 && gamma_denominator != 0.0) {
            const double delta_up = (level_two[2] - level_two[1]) / delta_up_denominator;
            const double delta_down = (level_two[1] - level_two[0]) / delta_down_denominator;
            gamma = (delta_up - delta_down) / gamma_denominator;
        }
    }

    const PricingResult output{values[0], delta, gamma, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0};
    const std::array result_values{output.value, output.delta, output.gamma};
    if (!std::ranges::all_of(result_values, [](double value) { return std::isfinite(value); })) {
        return std::unexpected(Error{error_category::invalid_result,
                                     "binomial pricing produced a non-finite result"});
    }
    return output;
}

}

namespace {

result<PricingResult> price_finite_difference(
    const EuropeanOption& option, const PricingContext& context,
    FiniteDifferenceSettings settings, bool american)
{
    const auto valid_expiry = validate_expiry(context.valuation_date(), option.expiry());
    if (!valid_expiry) return std::unexpected(valid_expiry.error());
    if (settings.asset_steps < 3 || settings.asset_steps > 10'000 ||
        settings.time_steps <= 0 || settings.time_steps > 100'000) {
        return std::unexpected(Error{error_category::invalid_parameter,
                                     "finite-difference grid dimensions are out of range"});
    }
    if (settings.upper_boundary != 0.0 &&
        (!std::isfinite(settings.upper_boundary) || settings.upper_boundary <= 0.0)) {
        return std::unexpected(Error{error_category::invalid_parameter,
                                     "finite-difference upper boundary must be finite and positive"});
    }
    switch (settings.scheme) {
    case finite_difference_scheme::explicit_euler:
    case finite_difference_scheme::implicit_euler:
    case finite_difference_scheme::crank_nicolson:
        break;
    default:
        return std::unexpected(Error{error_category::invalid_parameter,
                                     "finite-difference scheme is invalid"});
    }

    const double time = static_cast<double>(
        (option.expiry() - context.valuation_date()).count()) / days_per_year;
    const double spot = context.asset_price().value();
    const double strike = option.strike();
    const double sign = option.type() == option_type::call ? 1.0 : -1.0;
    if (time == 0.0) {
        return PricingResult{std::max(sign * (spot - strike), 0.0),
                             0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0};
    }

    const double rate = context.parameters().risk_free_rate();
    const double dividend = context.parameters().dividend_yield();
    const double volatility = context.parameters().volatility();
    const int asset_steps = settings.asset_steps;
    const double upper = settings.upper_boundary > 0.0
                              ? settings.upper_boundary
                              : std::max(4.0 * strike, 4.0 * spot);
    if (!std::isfinite(upper) || upper <= std::max(spot, strike)) {
        return std::unexpected(Error{error_category::invalid_parameter,
                                     "finite-difference upper boundary must exceed spot and strike"});
    }
    const double spacing = upper / static_cast<double>(asset_steps);
    int time_steps = settings.time_steps;
    if (settings.scheme == finite_difference_scheme::explicit_euler) {
        const double required = time * volatility * volatility * asset_steps * asset_steps * 1.1;
        if (!std::isfinite(required) || required > 100'000.0)
            return std::unexpected(Error{error_category::invalid_parameter,
                                         "explicit finite-difference grid requires too many time steps"});
        if (required > static_cast<double>(time_steps))
            time_steps = static_cast<int>(std::ceil(required));
    }
    const double dt = time / static_cast<double>(time_steps);
    const double theta = settings.scheme == finite_difference_scheme::explicit_euler ? 0.0
                         : settings.scheme == finite_difference_scheme::implicit_euler ? 1.0 : 0.5;

    auto boundary = [&](double tau, bool high) {
        if (high) {
            if (option.type() == option_type::call)
                return american ? upper - strike : upper * std::exp(-dividend * tau) -
                                             strike * std::exp(-rate * tau);
            return 0.0;
        }
        if (option.type() == option_type::put)
            return american ? strike : strike * std::exp(-rate * tau);
        return 0.0;
    };
    auto intrinsic = [&](double underlying) {
        return std::max(sign * (underlying - strike), 0.0);
    };

    std::vector<double> old(static_cast<std::size_t>(asset_steps) + 1);
    std::vector<double> next(old.size());
    for (int index = 0; index <= asset_steps; ++index)
        old[static_cast<std::size_t>(index)] = intrinsic(spacing * index);

    std::vector<double> lower(static_cast<std::size_t>(asset_steps) - 1);
    std::vector<double> diagonal(lower.size());
    std::vector<double> upper_diagonal(lower.size());
    std::vector<double> rhs(lower.size());
    auto solve = [&]() -> bool {
        for (std::size_t index = 1; index < diagonal.size(); ++index) {
            if (!std::isfinite(diagonal[index - 1]) || diagonal[index - 1] == 0.0) return false;
            const double factor = lower[index] / diagonal[index - 1];
            diagonal[index] -= factor * upper_diagonal[index - 1];
            rhs[index] -= factor * rhs[index - 1];
        }
        if (diagonal.empty() || !std::isfinite(diagonal.back()) || diagonal.back() == 0.0) return false;
        rhs.back() /= diagonal.back();
        for (std::size_t index = diagonal.size() - 1; index-- > 0;)
            rhs[index] = (rhs[index] - upper_diagonal[index] * rhs[index + 1]) / diagonal[index];
        return std::ranges::all_of(rhs, [](double value) { return std::isfinite(value); });
    };

    for (int step = 0; step < time_steps; ++step) {
        const double old_tau = static_cast<double>(step) * dt;
        const double new_tau = old_tau + dt;
        next.front() = boundary(new_tau, false);
        next.back() = boundary(new_tau, true);
        for (int index = 1; index < asset_steps; ++index) {
            const double i = static_cast<double>(index);
            const double a = 0.5 * volatility * volatility * i * i - 0.5 * (rate - dividend) * i;
            const double b = -volatility * volatility * i * i - rate;
            const double c = 0.5 * volatility * volatility * i * i + 0.5 * (rate - dividend) * i;
            const auto position = static_cast<std::size_t>(index - 1);
            rhs[position] = old[static_cast<std::size_t>(index)] + (1.0 - theta) * dt *
                            (a * old[static_cast<std::size_t>(index - 1)] +
                             b * old[static_cast<std::size_t>(index)] +
                             c * old[static_cast<std::size_t>(index + 1)]);
            if (index == 1) rhs[position] += theta * dt * a * next.front();
            if (index == asset_steps - 1)
                rhs[position] += theta * dt * c * next.back();
            lower[position] = -theta * dt * a;
            diagonal[position] = 1.0 - theta * dt * b;
            upper_diagonal[position] = -theta * dt * c;
        }
        if (theta == 0.0) {
            for (std::size_t index = 0; index < rhs.size(); ++index)
                next[index + 1] = rhs[index];
        } else {
            const bool solved = solve();
            if (!solved) {
                return std::unexpected(Error{error_category::invalid_result,
                                             "finite-difference system is numerically unstable"});
            }
            for (std::size_t index = 0; index < rhs.size(); ++index)
                next[index + 1] = rhs[index];
        }
        if (american) {
            for (int index = 1; index < asset_steps; ++index)
                next[static_cast<std::size_t>(index)] =
                    std::max(next[static_cast<std::size_t>(index)], intrinsic(spacing * index));
        }
        old.swap(next);
    }

    const double grid_position = spot / spacing;
    const int index = std::clamp(static_cast<int>(std::floor(grid_position)), 1, asset_steps - 1);
    const double weight = grid_position - static_cast<double>(index);
    const auto center = static_cast<std::size_t>(index);
    const double value = old[center] + weight * (old[center + 1] - old[center]);
    const double delta = (old[center + 1] - old[center - 1]) / (2.0 * spacing);
    const double gamma = (old[center + 1] - 2.0 * old[center] + old[center - 1]) /
                         (spacing * spacing);
    const PricingResult output{value, delta, gamma, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0};
    const std::array values{output.value, output.delta, output.gamma};
    if (!std::ranges::all_of(values, [](double item) { return std::isfinite(item); }))
        return std::unexpected(Error{error_category::invalid_result,
                                     "finite-difference pricing produced a non-finite result"});
    return output;
}

}

result<PricingResult> AnalyticEuropeanEngine::price(
    const EuropeanOption& option, const PricingContext& context) const
{
    return price_at_volatility(option, context, context.parameters().volatility());
}

result<PricingResult> BinomialAmericanEngine::price(
    const EuropeanOption& option, const PricingContext& context) const
{
    return price_binomial_american(option, context, settings_);
}

result<PricingResult> BinomialAmericanEngine::price(
    const EuropeanOption& option, const PricingContext& context,
    BinomialAmericanSettings settings) const
{
    return price_binomial_american(option, context, settings);
}

result<PricingResult> FiniteDifferenceEuropeanEngine::price(
    const EuropeanOption& option, const PricingContext& context) const
{
    return price_finite_difference(option, context, settings_, false);
}

result<PricingResult> FiniteDifferenceEuropeanEngine::price(
    const EuropeanOption& option, const PricingContext& context,
    FiniteDifferenceSettings settings) const
{
    return price_finite_difference(option, context, settings, false);
}

result<PricingResult> FiniteDifferenceAmericanEngine::price(
    const EuropeanOption& option, const PricingContext& context) const
{
    return price_finite_difference(option, context, settings_, true);
}

result<PricingResult> FiniteDifferenceAmericanEngine::price(
    const EuropeanOption& option, const PricingContext& context,
    FiniteDifferenceSettings settings) const
{
    return price_finite_difference(option, context, settings, true);
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

namespace {

PricingResult zero_tail(double value, double delta = 0.0, double gamma = 0.0)
{
    return PricingResult{value, delta, gamma, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0};
}

result<PricingResult> digital_price(double strike, option_type type, double payout,
                                    bool asset, date expiry, const PricingContext& context)
{
    const auto valid = validate_expiry(context.valuation_date(), expiry);
    if (!valid) return std::unexpected(valid.error());
    const double spot = context.asset_price().value();
    const double t = static_cast<double>((expiry - context.valuation_date()).count()) / days_per_year;
    const double sign = type == option_type::call ? 1.0 : -1.0;
    if (t == 0.0) {
        const bool exercised = sign * (spot - strike) > 0.0;
        return zero_tail(exercised ? (asset ? spot : payout) : 0.0);
    }
    const double sigma = context.parameters().volatility();
    const double root_t = std::sqrt(t);
    const double rate_df = std::exp(-context.parameters().risk_free_rate() * t);
    const double div_df = std::exp(-context.parameters().dividend_yield() * t);
    const double d1 = (std::log(spot / strike) +
                       (context.parameters().risk_free_rate() - context.parameters().dividend_yield() +
                        0.5 * sigma * sigma) * t) / (sigma * root_t);
    const double d2 = d1 - sigma * root_t;
    const double nd = normal_cdf(sign * (asset ? d1 : d2));
    const double density = normal_pdf(asset ? d1 : d2);
    const double scale = asset ? spot * div_df : payout * rate_df;
    const double value = scale * nd;
    double delta = 0.0;
    double gamma = 0.0;
    if (asset) {
        delta = div_df * (nd + sign * density / (sigma * root_t));
        gamma = -div_df * sign * density * d1 / (spot * sigma * sigma * t) +
                div_df * sign * density / (spot * sigma * root_t);
    } else {
        delta = payout * rate_df * sign * density / (spot * sigma * root_t);
        gamma = -payout * rate_df * sign * density *
                (1.0 + d2 / (sigma * root_t)) / (spot * spot * sigma * root_t);
    }
    const auto output = zero_tail(value, delta, gamma);
    const std::array values{output.value, output.delta, output.gamma};
    if (!std::ranges::all_of(values, [](double item) { return std::isfinite(item); }))
        return std::unexpected(Error{error_category::invalid_result, "analytic pricing produced a non-finite result"});
    return output;
}

double simpson(const std::function<double(double)>& f, double a, double b, int n = 2048)
{
    // ponytail: fixed 2048-panel Simpson integration over +/-12 sigma; adaptive quadrature if parity needs tighter tails.
    if (b <= a) return 0.0;
    if (n % 2) ++n;
    const double h = (b - a) / n;
    double sum = f(a) + f(b);
    for (int i = 1; i < n; ++i) sum += (i % 2 ? 4.0 : 2.0) * f(a + i * h);
    return sum * h / 3.0;
}

double barrier_survival_density(double y, double boundary, bool upper, double drift, double variance, double t)
{
    const double x = upper ? boundary : -boundary;
    if ((upper && y >= boundary) || (!upper && y <= boundary)) return 0.0;
    const double mean = drift * t;
    const double sd = std::sqrt(variance * t);
    const auto normal = [sd](double z) {
        return std::exp(-0.5 * z * z) / (sd * std::sqrt(2.0 * std::numbers::pi));
    };
    const double reflected = std::exp(-2.0 * (upper ? -drift : drift) * x / variance);
    if (upper) return normal(y - mean) - reflected * normal(y - 2.0 * boundary - mean);
    return normal(y - mean) - reflected * normal(y - 2.0 * boundary - mean);
}

double barrier_hit_discount(double distance, bool upper, double drift, double variance, double t, double rate)
{
    if (t == 0.0) return 1.0;
    const double signed_drift = upper ? -drift : drift;
    const double discriminant = signed_drift * signed_drift + 2.0 * rate * variance;
    if (discriminant <= 0.0) return 0.0;
    return std::exp((-signed_drift - std::sqrt(discriminant)) * distance / variance);
}

}

result<PricingResult> AnalyticDigitalEngine::price(
    const CashOrNothingOption& option, const PricingContext& context) const
{ return digital_price(option.strike(), option.type(), option.payout(), false, option.expiry(), context); }

result<PricingResult> AnalyticDigitalEngine::price(
    const AssetOrNothingOption& option, const PricingContext& context) const
{ return digital_price(option.strike(), option.type(), 1.0, true, option.expiry(), context); }

result<PricingResult> AnalyticBarrierEngine::price(
    const BarrierOption& option, const PricingContext& context) const
{
    const auto valid = validate_expiry(context.valuation_date(), option.expiry());
    if (!valid) return std::unexpected(valid.error());
    if (option.observation() == observation_mode::scheduled) {
        for (const auto observation : option.observation_dates()) {
            if (observation < context.valuation_date() || observation > option.expiry() ||
                !context.calendar().is_trading_day(observation))
                return std::unexpected(Error{error_category::invalid_schedule, "observation date is not a trading day"});
        }
        // ponytail: scheduled dates use a BGK barrier shift; exact discrete monitoring needs a separate engine.
    }
    const auto vanilla = price_at_volatility(
        *make_european_option(option.type(), option.strike(), option.expiry()), context,
        context.parameters().volatility());
    if (!vanilla) return std::unexpected(vanilla.error());
    const double t = static_cast<double>((option.expiry() - context.valuation_date()).count()) / days_per_year;
    const double spot = context.asset_price().value();
    const double rate = context.parameters().risk_free_rate();
    const double dividend = context.parameters().dividend_yield();
    const double sigma = context.parameters().volatility();
    double barrier = option.barrier();
    const bool upper = option.kind() == barrier_type::up_and_in || option.kind() == barrier_type::up_and_out;
    const bool knock_in = option.kind() == barrier_type::up_and_in || option.kind() == barrier_type::down_and_in;
    if (option.observation() == observation_mode::scheduled) {
        const double interval = t / static_cast<double>(option.observation_dates().size());
        barrier *= std::exp((upper ? 1.0 : -1.0) * 0.5825971579 * sigma * std::sqrt(interval));
    }
    const bool touched = upper ? spot >= barrier : spot <= barrier;
    if (t == 0.0) {
        if (knock_in) return zero_tail(touched ? vanilla->value : option.rebate());
        return zero_tail(touched ? option.rebate() : vanilla->value);
    }
    const double drift = rate - dividend - 0.5 * sigma * sigma;
    const double variance = sigma * sigma;
    const double boundary = std::log(barrier / spot);
    double survival_value = 0.0;
    double survival_probability = 0.0;
    if (!touched) {
        const double sd = sigma * std::sqrt(t);
        const double mean = drift * t;
        const double lo = mean - 12.0 * sd;
        const double hi = mean + 12.0 * sd;
        const double lower = upper ? lo : std::max(lo, boundary);
        const double higher = upper ? std::min(hi, boundary) : hi;
        const double sign = option.type() == option_type::call ? 1.0 : -1.0;
        const auto density = [&](double y) { return barrier_survival_density(y, boundary, upper, drift, variance, t); };
        survival_probability = simpson(density, lower, higher);
        survival_value = simpson([&](double y) {
            return std::max(sign * (spot * std::exp(y) - option.strike()), 0.0) * density(y) * std::exp(-rate * t);
        }, lower, higher);
    }
    double value = knock_in ? vanilla->value - survival_value : survival_value;
    if (knock_in && option.rebate() > 0.0 && !touched) {
        value += option.rebate() * std::exp(-rate * t) * survival_probability;
    }
    if (!knock_in && option.rebate() > 0.0) {
        double rebate_factor = std::exp(-rate * t) * (1.0 - survival_probability);
        if (option.rebate_payment() == rebate_timing::at_hit && !touched) {
            const double distance = std::abs(boundary);
            rebate_factor = barrier_hit_discount(distance, upper, drift, variance, t, rate) -
                            std::exp(-rate * t) * survival_probability;
        }
        value += option.rebate() * (touched ? 1.0 : rebate_factor);
    }
    if (!std::isfinite(value))
        return std::unexpected(Error{error_category::invalid_result, "analytic pricing produced a non-finite result"});
    return zero_tail(value);
}

}
