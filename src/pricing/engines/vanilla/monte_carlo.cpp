#include <kiyosi/pricing/engines/vanilla/monte_carlo.hpp>

#include <algorithm>
#include <array>
#include <cmath>
#include <new>
#include <random>
#include <ranges>
#include <vector>

#include <kiyosi/core/day_count.hpp>

#if KIYOSI_HAS_CUDA
#include "monte_carlo_cuda.hpp"
#endif

namespace kiyosi {
namespace {

constexpr int maximum_path_count = 10'000'000;
constexpr int maximum_step_count = 10'000;

enum class PathRetention { full,
                           terminal };

struct SimulationParameters {
    double spot;
    double rate;
    double drift;
    double diffusion;
};

result<double> simulation_time(const PricingContext& context, date effective, date expiry)
{
    const auto valid = validate_life(context.valuation_time(), effective, expiry);
    if (!valid) return std::unexpected(valid.error());
    const auto time = year_fraction(context.valuation_time(), start_of_day(expiry));
    if (!time || !std::isfinite(*time) || *time < 0.0)
        return std::unexpected(Error{error_category::invalid_expiry, "expiry produces an invalid simulation time"});
    return *time;
}

result<void> validate_settings(MonteCarloSettings settings)
{
    if (settings.path_count <= 0 || settings.path_count > maximum_path_count)
        return std::unexpected(Error{error_category::invalid_parameter,
                                     "Monte Carlo path count is out of range"});
    if (settings.step_count < 2 || settings.step_count > maximum_step_count)
        return std::unexpected(Error{error_category::invalid_parameter,
                                     "Monte Carlo step count is out of range"});
    if (settings.backend != monte_carlo_backend::cpu &&
        settings.backend != monte_carlo_backend::cuda)
        return std::unexpected(Error{error_category::invalid_parameter,
                                     "Monte Carlo backend is invalid"});
    return {};
}

result<SimulationParameters> simulation_parameters(
    const PricingContext& context, double time, MonteCarloSettings settings)
{
    const auto valid = validate_settings(settings);
    if (!valid) return std::unexpected(valid.error());
    const double volatility = context.parameters().volatility();
    const double dt = time / static_cast<double>(settings.step_count - 1);
    const double sqrt_dt = std::sqrt(dt);
    const double drift = (context.parameters().risk_free_rate() -
                          context.parameters().dividend_yield() -
                          0.5 * volatility * volatility) * dt;
    const double diffusion = volatility * sqrt_dt;
    if (!std::isfinite(dt) || !std::isfinite(sqrt_dt) || !std::isfinite(drift) ||
        !std::isfinite(diffusion))
        return std::unexpected(Error{error_category::invalid_result,
                                     "Monte Carlo simulation parameters are non-finite"});
    return SimulationParameters{context.asset_price(), context.parameters().risk_free_rate(),
                                drift, diffusion};
}

result<std::vector<double>> simulate_paths(
    SimulationParameters parameters, MonteCarloSettings settings, PathRetention retention)
{

    const int path_count = settings.path_count % 2 == 0 ? settings.path_count : settings.path_count + 1;
    const auto stride = retention == PathRetention::full
                            ? static_cast<std::size_t>(settings.step_count)
                            : std::size_t{1};
    const auto size = static_cast<std::size_t>(path_count) * stride;
    std::vector<double> paths(size);

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
        const auto positive = static_cast<std::size_t>(path) * stride;
        const auto negative = static_cast<std::size_t>(path + half_count) * stride;
        if (retention == PathRetention::full) {
            paths[positive] = parameters.spot;
            paths[negative] = parameters.spot;
        }
        double positive_spot = parameters.spot;
        double negative_spot = parameters.spot;
        for (int step = 1; step < settings.step_count; ++step) {
            const double normal_draw = normal(generator);
            positive_spot *= std::exp(parameters.drift + parameters.diffusion * normal_draw);
            negative_spot *= std::exp(parameters.drift - parameters.diffusion * normal_draw);
            if (!std::isfinite(positive_spot) || positive_spot <= 0.0 ||
                !std::isfinite(negative_spot) || negative_spot <= 0.0)
                return std::unexpected(Error{error_category::invalid_result,
                                             "Monte Carlo simulation produced a non-finite path"});
            if (retention == PathRetention::full) {
                paths[positive + static_cast<std::size_t>(step)] = positive_spot;
                paths[negative + static_cast<std::size_t>(step)] = negative_spot;
            }
        }
        if (retention == PathRetention::terminal) {
            paths[positive] = positive_spot;
            paths[negative] = negative_spot;
        }
    }
    return paths;
}

double payoff(option_type type, double spot, double strike)
{
    const double sign = type == option_type::call ? 1.0 : -1.0;
    return std::max(sign * (spot - strike), 0.0);
}

#if KIYOSI_HAS_CUDA
std::uint64_t random_seed()
{
    std::random_device source;
    return (static_cast<std::uint64_t>(source()) << 32U) ^
           static_cast<std::uint64_t>(source());
}

result<double> cuda_payoff_sum(const EuropeanOption& option,
                               SimulationParameters parameters,
                               MonteCarloSettings settings)
{
    const int path_count = settings.path_count % 2 == 0
                               ? settings.path_count
                               : settings.path_count + 1;
    const auto cuda_result = detail::cuda_european_price({
        path_count,
        settings.step_count,
        settings.seed ? *settings.seed : random_seed(),
        parameters.spot,
        option.strike(),
        parameters.drift,
        parameters.diffusion,
        option.type() == option_type::call ? 1 : -1,
    });
    switch (cuda_result.status) {
    case detail::CudaPricingStatus::success:
        return cuda_result.payoff_sum;
    case detail::CudaPricingStatus::unavailable:
        return std::unexpected(Error{error_category::backend_unavailable,
                                     cuda_result.message});
    case detail::CudaPricingStatus::failure:
        return std::unexpected(Error{error_category::backend_failure,
                                     cuda_result.message});
    case detail::CudaPricingStatus::out_of_memory:
        throw std::bad_alloc{};
    case detail::CudaPricingStatus::invalid_result:
        return std::unexpected(Error{error_category::invalid_result,
                                     cuda_result.message});
    }
    return std::unexpected(Error{error_category::backend_failure,
                                 "CUDA Monte Carlo returned an unknown status"});
}
#endif

using QuadraticRegressionMatrix = std::array<std::array<double, 4>, 3>;

bool solve_quadratic(QuadraticRegressionMatrix matrix,
                     std::array<double, 3>& coefficients)
{
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

result<PricingResult> MonteCarloVanillaEngine::price_european(
    const EuropeanOption& option, const PricingContext& context) const
{
    const auto time = simulation_time(context, option.effective(), option.expiry());
    if (!time) return std::unexpected(time.error());
    if (*time == 0.0)
        return make_pricing_result(
            {{risk_measure::price,
              payoff(option.type(), context.asset_price(), option.strike())}});
    const auto parameters = simulation_parameters(context, *time, settings_);
    if (!parameters) return std::unexpected(parameters.error());
    double sum = 0.0;
    std::size_t path_count = 0;
    if (settings_.backend == monte_carlo_backend::cuda) {
#if KIYOSI_HAS_CUDA
        const auto cuda_sum = cuda_payoff_sum(option, *parameters, settings_);
        if (!cuda_sum) return std::unexpected(cuda_sum.error());
        sum = *cuda_sum;
        path_count = static_cast<std::size_t>(
            settings_.path_count % 2 == 0 ? settings_.path_count : settings_.path_count + 1);
#else
        return std::unexpected(Error{error_category::backend_unavailable,
                                     "CUDA support is not enabled in this build"});
#endif
    } else {
        auto paths = simulate_paths(*parameters, settings_, PathRetention::terminal);
        if (!paths) return std::unexpected(paths.error());
        for (double terminal_spot : *paths)
            sum += payoff(option.type(), terminal_spot, option.strike());
        path_count = paths->size();
    }
    const double value = sum / static_cast<double>(path_count) *
                         std::exp(-parameters->rate * *time);
    if (!std::isfinite(value))
        return std::unexpected(Error{error_category::invalid_result, "Monte Carlo pricing produced a non-finite result"});
    return make_pricing_result({{risk_measure::price, value}});
}

result<PricingResult> MonteCarloVanillaEngine::price_american(
    const AmericanOption& option, const PricingContext& context) const
{
    if (settings_.backend == monte_carlo_backend::cuda)
        return std::unexpected(Error{error_category::unsupported_operation,
                                     "CUDA does not support American Monte Carlo pricing"});
    const auto time = simulation_time(context, option.effective(), option.expiry());
    if (!time) return std::unexpected(time.error());
    if (*time == 0.0)
        return make_pricing_result(
            {{risk_measure::price,
              payoff(option.type(), context.asset_price(), option.strike())}});
    if (settings_.step_count < 3)
        return std::unexpected(Error{error_category::invalid_parameter,
                                     "American Monte Carlo requires at least three grid points"});
    const auto parameters = simulation_parameters(context, *time, settings_);
    if (!parameters) return std::unexpected(parameters.error());
    auto paths = simulate_paths(*parameters, settings_, PathRetention::full);
    if (!paths) return std::unexpected(paths.error());
    const auto path_count = paths->size() / static_cast<std::size_t>(settings_.step_count);
    const double discount = std::exp(-context.parameters().risk_free_rate() *
                                     *time / static_cast<double>(settings_.step_count - 1));
    std::vector<double> cash_flows(path_count);
    const auto stride = static_cast<std::size_t>(settings_.step_count);
    for (std::size_t path = 0; path < path_count; ++path)
        cash_flows[path] = payoff(option.type(), (*paths)[path * stride + stride - 1], option.strike());
    for (int step = settings_.step_count - 2; step >= 1; --step) {
        for (double& value : cash_flows) value *= discount;
        QuadraticRegressionMatrix matrix{};
        std::size_t sample_count = 0;
        for (std::size_t path = 0; path < path_count; ++path) {
            const double spot = (*paths)[path * stride + static_cast<std::size_t>(step)];
            if (payoff(option.type(), spot, option.strike()) > 0.0) {
                ++sample_count;
                const double scaled = spot / option.strike();
                const double basis[] = {1.0, scaled, scaled * scaled};
                for (int row = 0; row < 3; ++row) {
                    for (int column = 0; column < 3; ++column)
                        matrix[row][column] += basis[row] * basis[column];
                    matrix[row][3] += basis[row] * cash_flows[path];
                }
            }
        }
        if (sample_count <= 2) continue;
        std::array<double, 3> coefficients{};
        if (!solve_quadratic(matrix, coefficients)) continue;
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
    const double continuation = sum / static_cast<double>(path_count) * discount;
    const double value = std::max(continuation,
        payoff(option.type(), context.asset_price(), option.strike()));
    if (!std::isfinite(value))
        return std::unexpected(Error{error_category::invalid_result, "Monte Carlo pricing produced a non-finite result"});
    return make_pricing_result({{risk_measure::price, value}});
}

} // namespace kiyosi
