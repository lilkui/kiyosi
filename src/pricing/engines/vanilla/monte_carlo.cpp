#include <kiyosi/pricing/engines/vanilla/monte_carlo.hpp>

#include <algorithm>
#include <array>
#include <cmath>
#include <cstdint>
#include <random>
#include <vector>

#include <kiyosi/core/day_count.hpp>

#include "monte_carlo_regression.hpp"
#include "../monte_carlo_mean.hpp"

#if KIYOSI_HAS_CUDA
#include "../monte_carlo_cuda_host.hpp"
#endif

namespace kiyosi {
namespace {

enum class PathRetention : std::uint8_t { full,
                                          terminal };

struct SimulationParameters {
    double spot;
    double rate;
    double drift;
    double diffusion;
};

Result<double> simulation_time(const PricingContext& context, Date effective_date, Date expiry_date)
{
    const auto valid = validate_valuation_within_instrument_life(context.valuation_time(), effective_date, expiry_date);
    if (!valid) return std::unexpected(valid.error());
    return detail::actual_365_fixed_year_fraction(context.valuation_time(), expiry_date);
}

Result<SimulationParameters> simulation_parameters(
    const PricingContext& context, double time, MonteCarloSettings settings)
{
    const auto valid = detail::validate_monte_carlo_settings(settings);
    if (!valid) return std::unexpected(valid.error());
    const double volatility = context.model_parameters().volatility();
    const double dt = time / static_cast<double>(settings.step_count - 1);
    const double sqrt_dt = std::sqrt(dt);
    const double drift = (context.model_parameters().risk_free_rate() -
                          context.model_parameters().dividend_yield() -
                          0.5 * volatility * volatility) *
                         dt;
    const double diffusion = volatility * sqrt_dt;
    if (!std::isfinite(dt) || !std::isfinite(sqrt_dt) || !std::isfinite(drift) ||
        !std::isfinite(diffusion))
        return std::unexpected(Error{ErrorCategory::invalid_result,
                                     "Monte Carlo simulation parameters are non-finite"});
    return SimulationParameters{context.spot_price(), context.model_parameters().risk_free_rate(),
                                drift, diffusion};
}

Result<std::vector<double>> simulate_paths(
    SimulationParameters parameters, MonteCarloSettings settings, PathRetention retention)
{

    const int path_count = settings.path_count % 2 == 0 ? settings.path_count : settings.path_count + 1;
    const auto stride = retention == PathRetention::full
                            ? static_cast<std::size_t>(settings.step_count)
                            : std::size_t{1};
    const auto size = static_cast<std::size_t>(path_count) * stride;
    std::vector<double> paths(size);

    std::mt19937_64 generator = [&] {
        if (settings.seed) return std::mt19937_64{*settings.seed};
        std::random_device source;
        std::seed_seq seed{source(), source(), source(), source()};
        return std::mt19937_64{seed};
    }();
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
                return std::unexpected(Error{ErrorCategory::invalid_result,
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

double payoff(OptionType type, double spot, double strike)
{
    const double sign = type == OptionType::call ? 1.0 : -1.0;
    return std::max(sign * (spot - strike), 0.0);
}

#if KIYOSI_HAS_CUDA
Result<double> cuda_payoff_mean(const EuropeanOption& option,
                                SimulationParameters parameters,
                                MonteCarloSettings settings)
{
    const int path_count = settings.path_count % 2 == 0
                               ? settings.path_count
                               : settings.path_count + 1;
    return detail::cuda_mean(detail::cuda_european_price({
        path_count,
        settings.step_count,
        settings.seed ? *settings.seed : detail::random_seed(),
        parameters.spot,
        option.strike(),
        parameters.drift,
        parameters.diffusion,
        option.option_type() == OptionType::call ? 1 : -1,
    }));
}

Result<double> cuda_american_cash_flow_mean(const AmericanOption& option,
                                            SimulationParameters parameters,
                                            MonteCarloSettings settings,
                                            double discount)
{
    const int path_count = settings.path_count % 2 == 0
                               ? settings.path_count
                               : settings.path_count + 1;
    return detail::cuda_mean(detail::cuda_american_price({
        path_count,
        settings.step_count,
        settings.seed ? *settings.seed : detail::random_seed(),
        parameters.spot,
        option.strike(),
        parameters.drift,
        parameters.diffusion,
        discount,
        option.option_type() == OptionType::call ? 1 : -1,
    }));
}
#endif

} // namespace

Result<PricingResult> MonteCarloVanillaEngine::price_native(
    const EuropeanOption& option, const PricingContext& context) const
{
    const auto time = simulation_time(context, option.effective_date(), option.expiry_date());
    if (!time) return std::unexpected(time.error());
    const auto valid = detail::validate_monte_carlo_settings(settings_);
    if (!valid) return std::unexpected(valid.error());
    if (*time == 0.0)
        return make_pricing_result(payoff(option.option_type(), context.spot_price(), option.strike()));
    auto simulation = settings_;
    simulation.step_count = 2;
    const auto parameters = simulation_parameters(context, *time, simulation);
    if (!parameters) return std::unexpected(parameters.error());
    double mean = 0.0;
    if (settings_.backend == MonteCarloBackend::cuda) {
#if KIYOSI_HAS_CUDA
        const auto cuda_result = cuda_payoff_mean(option, *parameters, simulation);
        if (!cuda_result) return std::unexpected(cuda_result.error());
        mean = *cuda_result;
#else
        return std::unexpected(Error{ErrorCategory::backend_unavailable,
                                     "CUDA support is not enabled in this build"});
#endif
    } else {
        auto paths = simulate_paths(*parameters, simulation, PathRetention::terminal);
        if (!paths) return std::unexpected(paths.error());
        detail::MonteCarloMean payoffs{};
        for (const double terminal_spot : *paths)
            payoffs.add(payoff(option.option_type(), terminal_spot, option.strike()));
        mean = payoffs.value();
    }
    const double value = mean *
                         std::exp(-parameters->rate * *time);
    if (!std::isfinite(value))
        return std::unexpected(Error{ErrorCategory::invalid_result, "Monte Carlo pricing produced a non-finite result"});
    return make_pricing_result(value);
}

Result<PricingResult> MonteCarloVanillaEngine::price_native(
    const AmericanOption& option, const PricingContext& context) const
{
    const auto time = simulation_time(context, option.effective_date(), option.expiry_date());
    if (!time) return std::unexpected(time.error());
    const auto valid = detail::validate_monte_carlo_settings(settings_);
    if (!valid) return std::unexpected(valid.error());
    if (*time == 0.0)
        return make_pricing_result(payoff(option.option_type(), context.spot_price(), option.strike()));
    if (settings_.step_count < 3)
        return std::unexpected(Error{ErrorCategory::invalid_parameter,
                                     "American Monte Carlo requires at least three grid points"});
    const auto parameters = simulation_parameters(context, *time, settings_);
    if (!parameters) return std::unexpected(parameters.error());
    const double discount = std::exp(-context.model_parameters().risk_free_rate() *
                                     *time / static_cast<double>(settings_.step_count - 1));
    double mean = 0.0;
    std::vector<double> cash_flows;
    if (settings_.backend == MonteCarloBackend::cuda) {
#if KIYOSI_HAS_CUDA
        const auto cuda_result =
            cuda_american_cash_flow_mean(option, *parameters, settings_, discount);
        if (!cuda_result) return std::unexpected(cuda_result.error());
        mean = *cuda_result;
#else
        return std::unexpected(Error{ErrorCategory::backend_unavailable,
                                     "CUDA support is not enabled in this build"});
#endif
    } else {
        auto paths = simulate_paths(*parameters, settings_, PathRetention::full);
        if (!paths) return std::unexpected(paths.error());
        const std::size_t path_count = paths->size() / static_cast<std::size_t>(settings_.step_count);
        cash_flows.resize(path_count);
        const auto stride = static_cast<std::size_t>(settings_.step_count);
        for (std::size_t path = 0; path < path_count; ++path)
            cash_flows[path] = payoff(
                option.option_type(), (*paths)[path * stride + stride - 1], option.strike());
        for (int step = settings_.step_count - 2; step >= 1; --step) {
            for (double& value : cash_flows)
                value *= discount;
            detail::QuadraticRegressionMatrix matrix{};
            std::size_t sample_count = 0;
            for (std::size_t path = 0; path < path_count; ++path) {
                const double spot = (*paths)[path * stride + static_cast<std::size_t>(step)];
                if (payoff(option.option_type(), spot, option.strike()) > 0.0) {
                    ++sample_count;
                    const double scaled = spot / option.strike();
                    const std::array<double, 3> basis{1.0, scaled, scaled * scaled};
                    for (int row = 0; row < 3; ++row) {
                        for (int column = 0; column < 3; ++column)
                            matrix[row][column] += basis[row] * basis[column];
                        matrix[row][3] += basis[row] * (cash_flows[path] / option.strike());
                    }
                }
            }
            if (sample_count == 0) continue;
            std::array<double, 3> coefficients{};
            if (!detail::solve_quadratic(matrix, coefficients)) continue;
            for (std::size_t path = 0; path < path_count; ++path) {
                const double spot = (*paths)[path * stride + static_cast<std::size_t>(step)];
                const double intrinsic = payoff(option.option_type(), spot, option.strike());
                if (intrinsic <= 0.0) continue;
                const double scaled = spot / option.strike();
                const double continuation =
                    coefficients[0] + scaled * (coefficients[1] + scaled * coefficients[2]);
                if (std::isfinite(continuation) && intrinsic / option.strike() > continuation)
                    cash_flows[path] = intrinsic;
            }
        }
        detail::MonteCarloMean payoffs{};
        for (const double cash_flow : cash_flows)
            payoffs.add(cash_flow);
        mean = payoffs.value();
    }
    const double continuation = mean * discount;
    const double value = std::max(continuation,
                                  payoff(option.option_type(), context.spot_price(), option.strike()));
    if (!std::isfinite(value))
        return std::unexpected(Error{ErrorCategory::invalid_result, "Monte Carlo pricing produced a non-finite result"});
    return make_pricing_result(value);
}

} // namespace kiyosi
