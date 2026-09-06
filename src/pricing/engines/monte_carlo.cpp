#include <kiyosi/pricing/engines/monte_carlo.hpp>

#include <algorithm>
#include <array>
#include <cmath>
#include <random>
#include <ranges>
#include <span>
#include <vector>

namespace kiyosi {
namespace {

constexpr int maximum_path_count = 10'000'000;
constexpr int maximum_step_count = 10'000;

result<double> simulation_time(const PricingContext& context, date expiry)
{
    const auto valid = validate_expiry(context.valuation_time(), expiry);
    if (!valid) return std::unexpected(valid.error());
    if (context.valuation_date() == expiry) return 0.0;
    const auto time = year_fraction(context.valuation_time(), start_of_day(expiry));
    if (!time || !std::isfinite(*time) || *time < 0.0)
        return std::unexpected(Error{error_category::invalid_expiry, "expiry produces an invalid simulation time"});
    return *time;
}

result<std::vector<double>> simulate_paths(
    const PricingContext& context, double time, MonteCarloSettings settings)
{
    if (settings.path_count <= 0 || settings.path_count > maximum_path_count)
        return std::unexpected(Error{error_category::invalid_parameter,
                                     "Monte Carlo path count is out of range"});
    if (settings.step_count < 2 || settings.step_count > maximum_step_count)
        return std::unexpected(Error{error_category::invalid_parameter,
                                     "Monte Carlo step count is out of range"});

    const int path_count = settings.path_count % 2 == 0 ? settings.path_count : settings.path_count + 1;
    const auto size = static_cast<std::size_t>(path_count) * static_cast<std::size_t>(settings.step_count);
    std::vector<double> paths(size);
    const double spot = context.asset_price().value();
    const double rate = context.parameters().risk_free_rate();
    const double dividend = context.parameters().dividend_yield();
    const double volatility = context.parameters().volatility();
    const double dt = time / static_cast<double>(settings.step_count - 1);
    const double sqrt_dt = std::sqrt(dt);
    const double drift = (rate - dividend - 0.5 * volatility * volatility) * dt;
    if (!std::isfinite(dt) || !std::isfinite(sqrt_dt) || !std::isfinite(drift))
        return std::unexpected(Error{error_category::invalid_result,
                                     "Monte Carlo simulation parameters are non-finite"});

    std::mt19937_64 generator;
    if (settings.seed) {
        generator.seed(*settings.seed);
    } else {
        std::random_device source;
        std::seed_seq seed{source(), source(), source(), source()};
        generator.seed(seed);
    }
    std::normal_distribution<double> normal;
    const int half_count = path_count / 2;
    for (int path = 0; path < half_count; ++path) {
        const auto positive = static_cast<std::size_t>(path) * settings.step_count;
        const auto negative = static_cast<std::size_t>(path + half_count) * settings.step_count;
        paths[positive] = spot;
        paths[negative] = spot;
        double positive_spot = spot;
        double negative_spot = spot;
        for (int step = 1; step < settings.step_count; ++step) {
            const double normal_draw = normal(generator);
            positive_spot *= std::exp(drift + volatility * sqrt_dt * normal_draw);
            negative_spot *= std::exp(drift - volatility * sqrt_dt * normal_draw);
            paths[positive + static_cast<std::size_t>(step)] = positive_spot;
            paths[negative + static_cast<std::size_t>(step)] = negative_spot;
        }
    }
    if (!std::ranges::all_of(paths, [](double value) { return std::isfinite(value) && value > 0.0; }))
        return std::unexpected(Error{error_category::invalid_result,
                                     "Monte Carlo simulation produced a non-finite path"});
    return paths;
}

double payoff(option_type type, double spot, double strike)
{
    const double sign = type == option_type::call ? 1.0 : -1.0;
    return std::max(sign * (spot - strike), 0.0);
}

bool fit_quadratic(std::span<const double> regression_spots,
                   std::span<const double> regression_values, double strike,
                   std::array<double, 3>& coefficients)
{
    std::array<std::array<double, 4>, 3> matrix{};
    for (std::size_t index = 0; index < regression_spots.size(); ++index) {
        const double scaled = regression_spots[index] / strike;
        const double basis[] = {1.0, scaled, scaled * scaled};
        for (int row = 0; row < 3; ++row) {
            for (int column = 0; column < 3; ++column)
                matrix[row][column] += basis[row] * basis[column];
            matrix[row][3] += basis[row] * regression_values[index];
        }
    }
    for (int column = 0; column < 3; ++column) {
        int pivot = column;
        for (int row = column + 1; row < 3; ++row)
            if (std::abs(matrix[row][column]) > std::abs(matrix[pivot][column])) pivot = row;
        if (!std::isfinite(matrix[pivot][column]) ||
            std::abs(matrix[pivot][column]) <= 1e-14 * std::max(1.0, std::abs(matrix[pivot][3])))
            return false;
        std::swap(matrix[column], matrix[pivot]);
        for (int row = column + 1; row < 3; ++row) {
            const double factor = matrix[row][column] / matrix[column][column];
            for (int entry = column; entry <= 3; ++entry)
                matrix[row][entry] -= factor * matrix[column][entry];
        }
    }
    for (int row = 2; row >= 0; --row) {
        double value = matrix[row][3];
        for (int column = row + 1; column < 3; ++column) value -= matrix[row][column] * coefficients[column];
        coefficients[row] = value / matrix[row][row];
    }
    return std::ranges::all_of(coefficients, [](double value) { return std::isfinite(value); });
}

} // namespace

result<PricingResult> MonteCarloEuropeanEngine::price_impl(
    const EuropeanOption& option, const PricingContext& context) const
{
    const auto time = simulation_time(context, option.expiry());
    if (!time) return std::unexpected(time.error());
    if (*time == 0.0)
        return PricingResult{{risk_measure::price, payoff(option.type(), context.asset_price().value(), option.strike())}};
    auto paths = simulate_paths(context, *time, settings_);
    if (!paths) return std::unexpected(paths.error());
    const auto path_count = paths->size() / static_cast<std::size_t>(settings_.step_count);
    double sum = 0.0;
    for (std::size_t path = 0; path < path_count; ++path)
        sum += payoff(option.type(), (*paths)[path * settings_.step_count + settings_.step_count - 1], option.strike());
    const double value = sum / static_cast<double>(path_count) * std::exp(-context.parameters().risk_free_rate() * *time);
    if (!std::isfinite(value))
        return std::unexpected(Error{error_category::invalid_result, "Monte Carlo pricing produced a non-finite result"});
    return PricingResult{{risk_measure::price, value}};
}

result<PricingResult> MonteCarloAmericanEngine::price_impl(
    const AmericanOption& option, const PricingContext& context) const
{
    const auto time = simulation_time(context, option.expiry());
    if (!time) return std::unexpected(time.error());
    if (*time == 0.0)
        return PricingResult{{risk_measure::price, payoff(option.type(), context.asset_price().value(), option.strike())}};
    if (settings_.step_count < 3)
        return std::unexpected(Error{error_category::invalid_parameter,
                                     "American Monte Carlo requires at least three grid points"});
    auto paths = simulate_paths(context, *time, settings_);
    if (!paths) return std::unexpected(paths.error());
    const auto path_count = paths->size() / static_cast<std::size_t>(settings_.step_count);
    const double discount = std::exp(-context.parameters().risk_free_rate() *
                                     *time / static_cast<double>(settings_.step_count - 1));
    std::vector<double> cash_flows(path_count);
    std::vector<double> regression_spots;
    std::vector<double> regression_values;
    const auto stride = static_cast<std::size_t>(settings_.step_count);
    for (std::size_t path = 0; path < path_count; ++path)
        cash_flows[path] = payoff(option.type(), (*paths)[path * stride + stride - 1], option.strike());
    for (int step = settings_.step_count - 2; step >= 1; --step) {
        for (double& value : cash_flows) value *= discount;
        regression_spots.clear();
        regression_values.clear();
        for (std::size_t path = 0; path < path_count; ++path) {
            const double spot = (*paths)[path * stride + static_cast<std::size_t>(step)];
            if (payoff(option.type(), spot, option.strike()) > 0.0) {
                regression_spots.push_back(spot);
                regression_values.push_back(cash_flows[path]);
            }
        }
        if (regression_spots.size() <= 2) continue;
        std::array<double, 3> coefficients{};
        if (!fit_quadratic(regression_spots, regression_values, option.strike(), coefficients)) continue;
        for (std::size_t path = 0; path < path_count; ++path) {
            const double spot = (*paths)[path * stride + static_cast<std::size_t>(step)];
            const double intrinsic = payoff(option.type(), spot, option.strike());
            if (intrinsic <= 0.0) continue;
            const double scaled = spot / option.strike();
            const double continuation = coefficients[0] + scaled * (coefficients[1] + scaled * coefficients[2]);
            if (std::isfinite(continuation) && intrinsic > continuation) cash_flows[path] = intrinsic;
        }
    }
    double sum = 0.0;
    for (double value : cash_flows) sum += value;
    const double value = sum / static_cast<double>(path_count) * discount;
    if (!std::isfinite(value))
        return std::unexpected(Error{error_category::invalid_result, "Monte Carlo pricing produced a non-finite result"});
    return PricingResult{{risk_measure::price, value}};
}

} // namespace kiyosi
