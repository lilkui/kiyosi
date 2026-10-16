#include <kiyosi/pricing/engines/vanilla/monte_carlo.hpp>

#include <kiyosi/pricing/detail/numerical_greeks.hpp>
#include <kiyosi/pricing/implied.hpp>

#include <algorithm>
#include <array>
#include <cmath>
#include <cstdint>
#include <random>
#include <limits>
#include <vector>

#include <kiyosi/core/day_count.hpp>

#include "../../detail/math.hpp"
#include "monte_carlo_regression.hpp"
#include "../monte_carlo_mean.hpp"
#include "../monte_carlo_cuda.hpp"

#if KIYOSI_HAS_CUDA
#include "../monte_carlo_host.hpp"
#endif

namespace kiyosi {
namespace {

struct SimulationParameters {
    double spot;
    double rate;
    double drift;
    double diffusion;
};

struct AmericanMonteCarloSamples {
    bool cache_growth;
    std::vector<double> paths{};
    std::vector<double> cash_flows{};
    std::vector<std::array<double, 2>> growth{};
    double drift = std::numeric_limits<double>::quiet_NaN();
    double diffusion = std::numeric_limits<double>::quiet_NaN();
    int step_count{};
};

Result<SimulationParameters> simulation_parameters(
    const PricingContext& context, double time, MonteCarloSettings settings)
{
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

template <typename Observe>
Result<void> simulate_paths(
    SimulationParameters parameters, MonteCarloSettings settings, const Observe& observe,
    AmericanMonteCarloSamples* samples = nullptr)
{
    const int path_count = settings.path_count % 2 == 0 ? settings.path_count : settings.path_count + 1;
    std::mt19937_64 generator = [&] {
        if (settings.seed) return std::mt19937_64{*settings.seed};
        std::random_device source;
        std::seed_seq seed{source(), source(), source(), source()};
        return std::mt19937_64{seed};
    }();
    std::normal_distribution<double> normal;
    const int half_count = path_count / 2;
    const bool reuse = samples && samples->drift == parameters.drift &&
                       samples->diffusion == parameters.diffusion && samples->step_count == settings.step_count;
    if (samples && !reuse) {
        samples->step_count = 0;
        samples->growth.resize(static_cast<std::size_t>(half_count) * (settings.step_count - 1));
    }
    constexpr int batch_size = 256;
    std::array<double, batch_size> draws{};
    for (int path = 0; path < half_count; ++path) {
        // Separate terminal-only random generation from payoff arithmetic without changing draw order.
        if (settings.step_count == 2 && !reuse && path % batch_size == 0)
            for (int index = 0; index < std::min(batch_size, half_count - path); ++index)
                draws[index] = normal(generator);
        double positive_spot = parameters.spot;
        double negative_spot = parameters.spot;
        for (int step = 1; step < settings.step_count; ++step) {
            const auto index = static_cast<std::size_t>(path) * (settings.step_count - 1) + step - 1;
            const auto growth = [&] {
                if (reuse) return samples->growth[index];
                const double normal_draw = settings.step_count == 2 ? draws[path % batch_size] : normal(generator);
                const std::array factors{std::exp(parameters.drift + parameters.diffusion * normal_draw),
                                         std::exp(parameters.drift - parameters.diffusion * normal_draw)};
                if (samples) samples->growth[index] = factors;
                return factors;
            }();
            positive_spot *= growth[0];
            negative_spot *= growth[1];
            if (!std::isfinite(positive_spot) || positive_spot <= 0.0 ||
                !std::isfinite(negative_spot) || negative_spot <= 0.0)
                return std::unexpected(Error{ErrorCategory::invalid_result,
                                             "Monte Carlo simulation produced a non-finite path"});
            observe(path, path + half_count, step, positive_spot, negative_spot);
        }
    }
    if (samples) {
        samples->drift = parameters.drift;
        samples->diffusion = parameters.diffusion;
        samples->step_count = settings.step_count;
    }
    return {};
}

double payoff(OptionType type, double spot, double strike)
{
    const double sign = type == OptionType::call ? 1.0 : -1.0;
    return std::max(sign * (spot - strike), 0.0);
}

#if KIYOSI_HAS_CUDA
Result<double> cuda_payoff_mean(const EuropeanOption& option,
                                SimulationParameters parameters,
                                MonteCarloSettings settings, detail::CudaWorkspace* workspace)
{
    const int path_count = settings.path_count % 2 == 0
                               ? settings.path_count
                               : settings.path_count + 1;
    return detail::cuda_mean(detail::cuda_european_price({
                                                             path_count,
                                                             settings.seed ? *settings.seed : detail::random_seed(),
                                                             parameters.spot,
                                                             option.strike(),
                                                             parameters.drift,
                                                             parameters.diffusion,
                                                             option.option_type() == OptionType::call ? 1 : -1,
                                                         },
                                                         workspace));
}

Result<double> cuda_american_cash_flow_mean(const AmericanOption& option,
                                            SimulationParameters parameters,
                                            MonteCarloSettings settings,
                                            double discount, detail::CudaWorkspace* workspace, bool cache_growth)
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
                                                         },
                                                         workspace, cache_growth));
}
#endif

template <typename Simulate>
Result<double> price_european(const EuropeanOption& option, const PricingContext& context,
                              MonteCarloSettings settings, const Simulate& simulate,
                              [[maybe_unused]] detail::CudaWorkspace* workspace = nullptr)
{
    const auto time = detail::time_to_expiry(context.valuation_time(), option.effective_date(), option.expiry_date());
    if (!time) return std::unexpected(time.error());
    const auto valid = detail::validate_monte_carlo_settings(settings);
    if (!valid) return std::unexpected(valid.error());
    if (*time == 0.0)
        return detail::checked_price(payoff(option.option_type(), context.spot_price(), option.strike()));
    auto simulation = settings;
    simulation.step_count = 2;
    const auto parameters = simulation_parameters(context, *time, simulation);
    if (!parameters) return std::unexpected(parameters.error());
    double mean = 0.0;
    if (settings.backend == MonteCarloBackend::cuda) {
#if KIYOSI_HAS_CUDA
        const auto cuda_result = cuda_payoff_mean(option, *parameters, simulation, workspace);
        if (!cuda_result) return std::unexpected(cuda_result.error());
        mean = *cuda_result;
#else
        return std::unexpected(Error{ErrorCategory::backend_unavailable,
                                     "CUDA support is not enabled in this build"});
#endif
    } else {
        detail::MonteCarloMean payoffs{};
        const auto simulated = simulate(*parameters, simulation, [&](int, int, int, double positive, double negative) {
            payoffs.add(payoff(option.option_type(), positive, option.strike()));
            payoffs.add(payoff(option.option_type(), negative, option.strike()));
        });
        if (!simulated) return std::unexpected(simulated.error());
        mean = payoffs.value();
    }
    const double discount = std::exp(-parameters->rate * *time);
    // Combine scales when the discount alone loses precision or leaves the finite range.
    const double value = mean == 0.0               ? 0.0
                         : std::isnormal(discount) ? mean * discount
                                                   : std::exp(std::log(mean) - parameters->rate * *time);
    if (!std::isfinite(value))
        return std::unexpected(Error{ErrorCategory::invalid_result, "Monte Carlo pricing produced a non-finite result"});
    return value;
}

// Each request owns its samples; consecutive spot bumps reuse exact terminal growth factors.
class EuropeanMonteCarloSamples {
public:
    explicit EuropeanMonteCarloSamples(MonteCarloSettings settings) : settings_(settings) {}

    Result<double> price(const EuropeanOption& option, const PricingContext& context) const
    {
        detail::CudaWorkspace* workspace = nullptr;
#if KIYOSI_HAS_CUDA
        workspace = &workspace_;
#endif
        return price_european(option, context, settings_, [&](auto parameters, auto simulation, const auto& observe) {
            if (normal_draws_.empty()) {
                std::mt19937_64 generator{*simulation.seed};
                std::normal_distribution<double> normal;
                normal_draws_.resize(static_cast<std::size_t>((simulation.path_count + 1) / 2));
                for (double& draw : normal_draws_)
                    draw = normal(generator);
                growth_.resize(normal_draws_.size());
            }
            if (drift_ != parameters.drift || diffusion_ != parameters.diffusion) {
                for (std::size_t path = 0; path < normal_draws_.size(); ++path) {
                    growth_[path] = {std::exp(parameters.drift + parameters.diffusion * normal_draws_[path]),
                                     std::exp(parameters.drift - parameters.diffusion * normal_draws_[path])};
                }
                drift_ = parameters.drift;
                diffusion_ = parameters.diffusion;
            }
            const int half_count = static_cast<int>(normal_draws_.size());
            for (int path = 0; path < half_count; ++path) {
                const auto& growth = growth_[static_cast<std::size_t>(path)];
                const double positive = parameters.spot * growth[0];
                const double negative = parameters.spot * growth[1];
                if (!std::isfinite(positive) || positive <= 0.0 || !std::isfinite(negative) || negative <= 0.0)
                    return Result<void>{std::unexpected(Error{ErrorCategory::invalid_result,
                                                              "Monte Carlo simulation produced a non-finite path"})};
                observe(path, path + half_count, 1, positive, negative);
            }
            return Result<void>{}; }, workspace);
    }

private:
    MonteCarloSettings settings_;
    mutable std::vector<double> normal_draws_;
    mutable std::vector<std::array<double, 2>> growth_;
    mutable double drift_ = std::numeric_limits<double>::quiet_NaN();
    mutable double diffusion_ = std::numeric_limits<double>::quiet_NaN();
#if KIYOSI_HAS_CUDA
    mutable detail::CudaWorkspace workspace_;
#endif
};

} // namespace

template <>
Result<double> implied_volatility<MonteCarloVanillaEngine, EuropeanOption>(
    const MonteCarloVanillaEngine& engine, const EuropeanOption& option, const PricingContext& context,
    double observed_price, ImpliedVolatilitySettings settings)
{
    std::optional<EuropeanMonteCarloSamples> samples;
    return detail::solve_implied_volatility(engine, option, context, observed_price, settings,
                                            [&](const auto& seeded, const auto& instrument, const auto& shifted) {
                                                if (!samples) samples.emplace(seeded.settings());
                                                return samples->price(instrument, shifted);
                                            });
}

Result<double> MonteCarloVanillaEngine::price(const EuropeanOption& option, const PricingContext& context) const
{
    return price_european(option, context, settings_, [](auto parameters, auto simulation, const auto& observe) {
        return simulate_paths(parameters, simulation, observe);
    });
}

namespace {

Result<double> price_american(const AmericanOption& option, const PricingContext& context,
                              MonteCarloSettings settings, bool terminal_only,
                              [[maybe_unused]] detail::CudaWorkspace* workspace = nullptr,
                              AmericanMonteCarloSamples* samples = nullptr)
{
    const auto time = detail::time_to_expiry(context.valuation_time(), option.effective_date(), option.expiry_date());
    if (!time) return std::unexpected(time.error());
    const auto valid = detail::validate_monte_carlo_settings(settings);
    if (!valid) return std::unexpected(valid.error());
    if (*time == 0.0)
        return detail::checked_price(payoff(option.option_type(), context.spot_price(), option.strike()));
    if (settings.step_count < 3)
        return std::unexpected(Error{ErrorCategory::invalid_parameter,
                                     "American Monte Carlo requires at least three grid points"});
    if (terminal_only && option.option_type() == OptionType::call &&
        context.model_parameters().dividend_yield() <= 0.0 &&
        context.model_parameters().risk_free_rate() >= 0.0) {
        const auto european = *make_european_option(
            option.option_type(), option.strike(), option.effective_date(), option.expiry_date());
        const auto value = price_european(european, context, settings, [&](auto parameters, auto simulation, const auto& observe) { return simulate_paths(parameters, simulation, observe, samples && samples->cache_growth ? samples : nullptr); }, workspace);
        if (!value) return std::unexpected(value.error());
        return std::max(*value, payoff(option.option_type(), context.spot_price(), option.strike()));
    }
    const auto parameters = simulation_parameters(context, *time, settings);
    if (!parameters) return std::unexpected(parameters.error());
    const double discount = std::exp(-context.model_parameters().risk_free_rate() *
                                     *time / static_cast<double>(settings.step_count - 1));
    double mean = 0.0;
    if (settings.backend == MonteCarloBackend::cuda) {
#if KIYOSI_HAS_CUDA
        const auto cuda_result =
            cuda_american_cash_flow_mean(option, *parameters, settings, discount, workspace,
                                         samples && samples->cache_growth);
        if (!cuda_result) return std::unexpected(cuda_result.error());
        mean = *cuda_result;
#else
        return std::unexpected(Error{ErrorCategory::backend_unavailable,
                                     "CUDA support is not enabled in this build"});
#endif
    } else {
        const auto path_count = static_cast<std::size_t>(settings.path_count) + settings.path_count % 2;
        const auto step_count = static_cast<std::size_t>(settings.step_count);
        // Regression visits every path at one time, so keep each time slice contiguous.
        std::vector<double> owned_paths;
        std::vector<double> owned_cash_flows;
        auto& paths = samples ? samples->paths : owned_paths;
        auto& cash_flows = samples ? samples->cash_flows : owned_cash_flows;
        paths.resize(path_count * step_count);
        const auto simulated = simulate_paths(*parameters, settings, [&](int positive, int negative, int step, double positive_spot, double negative_spot) {
            paths[static_cast<std::size_t>(step) * path_count + static_cast<std::size_t>(positive)] = positive_spot;
            paths[static_cast<std::size_t>(step) * path_count + static_cast<std::size_t>(negative)] = negative_spot; }, samples && samples->cache_growth ? samples : nullptr);
        if (!simulated) return std::unexpected(simulated.error());
        cash_flows.resize(path_count);
        for (std::size_t path = 0; path < path_count; ++path)
            cash_flows[path] = payoff(
                option.option_type(), paths[(step_count - 1) * path_count + path], option.strike());
        for (int step = settings.step_count - 2; step >= 1; --step) {
            for (double& value : cash_flows)
                value *= discount;
            detail::QuadraticRegressionMatrix matrix{};
            std::size_t sample_count = 0;
            for (std::size_t path = 0; path < path_count; ++path) {
                const double spot = paths[static_cast<std::size_t>(step) * path_count + path];
                if (payoff(option.option_type(), spot, option.strike()) > 0.0) {
                    ++sample_count;
                    const double scaled = spot / option.strike();
                    const double square = scaled * scaled;
                    const double target = cash_flows[path] / option.strike();
                    matrix[0][0] += 1.0;
                    matrix[0][1] += scaled;
                    matrix[0][2] += square;
                    matrix[1][1] += square;
                    matrix[1][2] += scaled * square;
                    matrix[2][2] += square * square;
                    matrix[0][3] += target;
                    matrix[1][3] += scaled * target;
                    matrix[2][3] += square * target;
                }
            }
            if (sample_count == 0) continue;
            matrix[1][0] = matrix[0][1];
            matrix[2][0] = matrix[0][2];
            matrix[2][1] = matrix[1][2];
            std::array<double, 3> coefficients{};
            if (!detail::solve_quadratic(matrix, coefficients)) continue;
            for (std::size_t path = 0; path < path_count; ++path) {
                const double spot = paths[static_cast<std::size_t>(step) * path_count + path];
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
    return value;
}

class AmericanMonteCarloPricer {
public:
    explicit AmericanMonteCarloPricer(MonteCarloSettings settings, bool cache_growth = false)
        : settings_(settings), samples_{.cache_growth = cache_growth} {}

    Result<double> price(const AmericanOption& option, const PricingContext& context, bool terminal_only = true) const
    {
        detail::CudaWorkspace* workspace = nullptr;
#if KIYOSI_HAS_CUDA
        if (settings_.backend == MonteCarloBackend::cuda) workspace = &workspace_;
#endif
        return price_american(option, context, settings_, terminal_only, workspace, &samples_);
    }

private:
    MonteCarloSettings settings_;
    mutable AmericanMonteCarloSamples samples_;
#if KIYOSI_HAS_CUDA
    mutable detail::CudaWorkspace workspace_;
#endif
};

} // namespace

template <>
Result<double> implied_volatility<MonteCarloVanillaEngine, AmericanOption>(
    const MonteCarloVanillaEngine& engine, const AmericanOption& option, const PricingContext& context,
    double observed_price, ImpliedVolatilitySettings settings)
{
    std::optional<AmericanMonteCarloPricer> prices;
    return detail::solve_implied_volatility(engine, option, context, observed_price, settings,
                                            [&](const auto& seeded, const auto& instrument, const auto& shifted) {
                                                if (!prices) prices.emplace(seeded.settings());
                                                return prices->price(instrument, shifted);
                                            });
}

Result<double> MonteCarloVanillaEngine::price(const AmericanOption& option, const PricingContext& context) const
{
    return price_american(option, context, settings_, true);
}

Result<PricingResult> MonteCarloVanillaEngine::price_with_greeks(const EuropeanOption& option, const PricingContext& context,
                                                                 GreeksRequest greeks, NumericalShiftSettings settings) const
{
    return detail::evaluate_greeks(*this, option, context, greeks, settings, [&](const auto& seeded) -> Result<PricingResult> {
        const auto stencils = detail::numerical_stencils(option, context, settings);
        const bool cache_samples = greeks.has(Greek::theta) ||
            (stencils.volatility_available && greeks.has(Greek::vega)) || (stencils.rate_available && greeks.has(Greek::rho)) ||
            (stencils.spot_available &&
             (greeks.has(Greek::delta) || greeks.has(Greek::gamma) || greeks.has(Greek::charm) || greeks.has(Greek::color) ||
              (greeks.has(Greek::speed) && stencils.speed_available) ||
              (stencils.volatility_available && (greeks.has(Greek::vanna) || greeks.has(Greek::zomma)))));
        // Stream a single price without sample storage; shared completion still decides availability.
        if (!cache_samples)
            return detail::price_with_greeks(seeded, option, context, greeks, settings);
        const EuropeanMonteCarloSamples samples{seeded.settings()};
        const auto value = samples.price(option, context);
        if (!value) return std::unexpected(value.error());
        return detail::complete_greeks([&](const PricingContext& shifted) { return samples.price(option, shifted); }, option, context, greeks, settings, *make_pricing_result(*value)); });
}

Result<PricingResult> MonteCarloVanillaEngine::price_with_greeks(const AmericanOption& option, const PricingContext& context,
                                                                 GreeksRequest greeks, NumericalShiftSettings settings) const
{
    return detail::evaluate_greeks(*this, option, context, greeks, settings, [&](const auto& seeded) -> Result<PricingResult> {
        const auto stencils = detail::numerical_stencils(option, context, settings);
        const bool cache_growth = stencils.spot_available &&
            (greeks.has(Greek::delta) || greeks.has(Greek::gamma) || (greeks.has(Greek::speed) && stencils.speed_available) ||
             (stencils.volatility_available && (greeks.has(Greek::vanna) || greeks.has(Greek::zomma))) ||
             greeks.has(Greek::charm) || greeks.has(Greek::color));
        const AmericanMonteCarloPricer prices{seeded.settings(), cache_growth};
        const auto native = [&]() -> Result<PricingResult> {
        const auto value = prices.price(option, context);
        if (!value) return std::unexpected(value.error());
        const double rate = context.model_parameters().risk_free_rate();
        const double shift = stencils.rate_shift;
        if (greeks.has(Greek::rho) && stencils.rate_available && !detail::greeks_unavailable(option, context) &&
            option.option_type() == OptionType::call && context.model_parameters().dividend_yield() <= 0.0 &&
            rate - shift < 0.0 && rate + shift >= 0.0) {
            // Keep one estimator across the rate stencil when the terminal-only shortcut changes branch.
            const auto evaluate = [&](double shifted_rate) -> Result<double> {
                const auto shifted = detail::shifted_context(context, context.spot_price(),
                                                             context.model_parameters().volatility(), shifted_rate, context.valuation_time());
                if (!shifted) return std::unexpected(shifted.error());
                return prices.price(option, *shifted, false);
            };
            const auto high = evaluate(rate + shift);
            if (!high) return std::unexpected(high.error());
            const auto low = evaluate(rate - shift);
            if (!low) return std::unexpected(low.error());
            return make_pricing_result(*value, {{Greek::rho, (*high - *low) / (200.0 * shift)}});
        }
        return make_pricing_result(*value);
        }();
        if (!native) return std::unexpected(native.error());
        return detail::complete_greeks([&](const PricingContext& shifted) { return prices.price(option, shifted); }, option, context, greeks, settings, *native); });
}

} // namespace kiyosi
