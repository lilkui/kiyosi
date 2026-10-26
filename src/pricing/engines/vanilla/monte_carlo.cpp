#include "../monte_carlo_parallel.hpp"
#include "../monte_carlo_random.hpp"
#include <kiyosi/pricing/engines/vanilla/monte_carlo.hpp>

#include "../../detail/numerical_greeks.hpp"
#include <kiyosi/pricing/implied.hpp>

#include <algorithm>
#include <array>
#include <cassert>
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

#include "../monte_carlo_host.hpp"

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
    bool cache_normals{};
    std::vector<double> paths{};
    std::vector<double> cash_flows{};
    std::vector<double> growth{};
    std::vector<double> normal_draws{};
    int normal_step_count{};
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
    const double drift = ((*context.model_parameters().risk_free_rate()) -
                          (*context.model_parameters().dividend_yield()) -
                          0.5 * volatility * volatility) *
                         dt;
    const double diffusion = volatility * sqrt_dt;
    if (!std::isfinite(dt) || !std::isfinite(sqrt_dt) || !std::isfinite(drift) ||
        !std::isfinite(diffusion))
        return std::unexpected(Error{ErrorCategory::invalid_result,
                                     "Monte Carlo simulation parameters are non-finite"});
    return SimulationParameters{context.spot_price(), (*context.model_parameters().risk_free_rate()),
                                drift, diffusion};
}

template <typename Observe>
Result<void> simulate_paths(
    SimulationParameters parameters, MonteCarloSettings settings, const Observe& observe,
    AmericanMonteCarloSamples* samples = nullptr)
{
    const bool antithetic = settings.antithetic;
    const int path_count = settings.path_count;
    const auto seed = settings.seed ? *settings.seed : std::random_device{}();
    const int sample_count = antithetic ? (path_count + 1) / 2 : path_count;
    const std::size_t growth_stride = antithetic ? 2 : 1;
    const bool reuse = samples && samples->cache_growth && samples->drift == parameters.drift &&
                       samples->diffusion == parameters.diffusion && samples->step_count == settings.step_count;
    if (samples && samples->cache_growth && !reuse) {
        samples->step_count = 0;
        samples->growth.resize(static_cast<std::size_t>(sample_count) * (settings.step_count - 1) * growth_stride);
    }
    const bool reuse_normals = samples && samples->cache_normals && samples->normal_step_count == settings.step_count;
    if (samples && samples->cache_normals && !reuse_normals) {
        samples->normal_step_count = 0;
        samples->normal_draws.resize(static_cast<std::size_t>(sample_count) * (settings.step_count - 1));
    }
    std::optional<detail::SobolSequence> sobol;
    if (settings.sampling == MonteCarloSampling::sobol && !reuse && !reuse_normals)
        sobol.emplace(static_cast<std::size_t>(settings.step_count - 1), seed);
    const auto simulated = detail::monte_carlo_blocks(sample_count, settings.thread_count,
                                                      [&](int block, int begin, int end) -> Result<void> {
                                                          const auto simulate = [&](auto& generator) -> Result<void> {
                                                              std::normal_distribution<double> normal;
                                                              for (int path = begin; path < end; ++path) {
                                                                  const bool paired = antithetic && path < settings.path_count / 2;
                                                                  double positive_spot = parameters.spot;
                                                                  double negative_spot = parameters.spot;
                                                                  for (int step = 1; step < settings.step_count; ++step) {
                                                                      const auto index = static_cast<std::size_t>(path) * (settings.step_count - 1) + step - 1;
                                                                      const auto growth_index = index * growth_stride;
                                                                      const auto growth = [&] {
                                                                          if (reuse) return std::array{samples->growth[growth_index], samples->growth[growth_index + (antithetic ? 1 : 0)]};
                                                                          const double normal_draw = reuse_normals ? samples->normal_draws[index] : detail::draw_monte_carlo_normal(generator, normal);
                                                                          if (samples && samples->cache_normals && !reuse_normals) samples->normal_draws[index] = normal_draw;
                                                                          // Explicit fusion keeps streamed, cached and batched growth identical.
                                                                          const double positive = std::exp(std::fma(parameters.diffusion, normal_draw, parameters.drift));
                                                                          const std::array factors{positive, paired ? std::exp(std::fma(-parameters.diffusion, normal_draw, parameters.drift)) : positive};
                                                                          if (samples && samples->cache_growth) {
                                                                              samples->growth[growth_index] = factors[0];
                                                                              if (antithetic) samples->growth[growth_index + 1] = factors[1];
                                                                          }
                                                                          return factors;
                                                                      }();
                                                                      positive_spot *= growth[0];
                                                                      negative_spot *= growth[1];
                                                                      if (!std::isfinite(positive_spot) || positive_spot <= 0.0 ||
                                                                          !std::isfinite(negative_spot) || negative_spot <= 0.0)
                                                                          return std::unexpected(Error{ErrorCategory::invalid_result,
                                                                                                       "Monte Carlo simulation produced a non-finite path"});
                                                                      observe(path, paired ? path + sample_count : path, step, positive_spot, negative_spot);
                                                                  }
                                                                  if constexpr (std::same_as<std::remove_cvref_t<decltype(generator)>, detail::SobolCursor>) generator.next_path();
                                                              }
                                                              return {};
                                                          };
                                                          if (sobol) {
                                                              detail::SobolCursor generator{*sobol, static_cast<std::uint64_t>(begin)};
                                                              return simulate(generator);
                                                          }
                                                          detail::Pcg32 generator{seed, static_cast<std::uint64_t>(block)};
                                                          return simulate(generator);
                                                      });
    if (!simulated) return std::unexpected(simulated.error());
    if (samples) {
        samples->drift = parameters.drift;
        samples->diffusion = parameters.diffusion;
        samples->step_count = settings.step_count;
        if (samples->cache_normals) samples->normal_step_count = settings.step_count;
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
    return detail::cuda_mean(detail::cuda_european_price({
                                                             settings.path_count,
                                                             settings.seed ? *settings.seed : detail::random_seed(),
                                                             parameters.spot,
                                                             option.strike(),
                                                             parameters.drift,
                                                             parameters.diffusion,
                                                             option.option_type() == OptionType::call ? 1 : -1,
                                                             settings.antithetic,
                                                         },
                                                         workspace));
}

Result<double> cuda_american_cash_flow_mean(const AmericanOption& option,
                                            SimulationParameters parameters,
                                            MonteCarloSettings settings,
                                            double discount, detail::CudaWorkspace* workspace, bool cache_growth)
{
    return detail::cuda_mean(detail::cuda_american_price({
                                                             settings.path_count,
                                                             settings.step_count,
                                                             settings.seed ? *settings.seed : detail::random_seed(),
                                                             parameters.spot,
                                                             option.strike(),
                                                             parameters.drift,
                                                             parameters.diffusion,
                                                             discount,
                                                             option.option_type() == OptionType::call ? 1 : -1,
                                                             settings.antithetic,
                                                         },
                                                         workspace, cache_growth));
}
#endif

Result<double> discounted_european_mean(double mean, double rate, double time)
{
    const double discount = std::exp(-rate * time);
    // Combine scales when the discount alone loses precision or leaves the finite range.
    const double value = mean == 0.0               ? 0.0
                         : std::isnormal(discount) ? mean * discount
                                                   : std::exp(std::log(mean) - rate * time);
    if (!std::isfinite(value))
        return std::unexpected(Error{ErrorCategory::invalid_result, "Monte Carlo pricing produced a non-finite result"});
    return value;
}

template <typename Simulate>
Result<double> price_european(const EuropeanOption& option, const PricingContext& context,
                              MonteCarloSettings settings, const Simulate& simulate,
                              [[maybe_unused]] detail::CudaWorkspace* workspace = nullptr)
{
    const auto time = detail::time_to_expiry(context.valuation_time(), option.effective_date(), option.expiry_date());
    if (!time) return std::unexpected(time.error());
    if (*time == 0.0)
        return detail::checked_price(payoff(option.option_type(), context.spot_price(), option.strike()));
    auto simulation = settings;
    simulation.step_count = 2;
    // Small terminal samples do not amortize automatic scheduling across the available workers.
    if (simulation.backend == MonteCarloBackend::cpu && simulation.thread_count == 0 && simulation.path_count <= 3072)
        simulation.thread_count = 1;
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
        const bool antithetic = simulation.antithetic;
        std::vector<detail::MonteCarloMean> partials(static_cast<std::size_t>(
            detail::monte_carlo_block_count(antithetic ? (simulation.path_count + 1) / 2 : simulation.path_count)));
        const auto simulated = simulate(*parameters, simulation, [&](int path, int negative_path, int, double positive, double negative) {
            auto& partial = partials[static_cast<std::size_t>(path / detail::monte_carlo_block_size)];
            partial.add(payoff(option.option_type(), positive, option.strike()));
            if (negative_path != path) partial.add(payoff(option.option_type(), negative, option.strike()));
        });
        if (!simulated) return std::unexpected(simulated.error());
        detail::MonteCarloMean payoffs{};
        for (const auto& partial : partials)
            payoffs.merge(partial);
        mean = payoffs.value();
    }
    return discounted_european_mean(mean, parameters->rate, *time);
}

// Each request owns its samples; spot batches share exact terminal growth factors.
class EuropeanMonteCarloSamples {
public:
    explicit EuropeanMonteCarloSamples(MonteCarloSettings settings, bool cache_samples = true)
        : settings_(settings), cache_samples_(cache_samples) {}

    Result<double> price(const EuropeanOption& option, const PricingContext& context) const
    {
        if (settings_.backend == MonteCarloBackend::cuda) {
            detail::CudaWorkspace* workspace = nullptr;
#if KIYOSI_HAS_CUDA
            workspace = &workspace_;
#endif
            return price_european(option, context, settings_, [](auto parameters, auto simulation, const auto& observe) { return simulate_paths(parameters, simulation, observe); }, workspace);
        }
        const auto values = prices(option, context, std::array{context.spot_price()});
        if (!values) return std::unexpected(values.error());
        return (*values)[0];
    }

    template <std::size_t count>
    Result<std::array<double, count>> spot_prices(const EuropeanOption& option, const PricingContext& context, double h) const
    {
        std::array<double, count> spots{};
        for (std::size_t i = 0; i < count; ++i) {
            spots[i] = context.spot_price() + detail::SpotStencil<count>::offsets[i] * h;
            const auto shifted = detail::shifted_context(context, spots[i], context.model_parameters().volatility(),
                                                         (*context.model_parameters().risk_free_rate()), context.valuation_time());
            if (!shifted) return std::unexpected(shifted.error());
        }
        return prices(option, context, spots);
    }

private:
    template <std::size_t count>
    Result<std::array<double, count>> prices(const EuropeanOption& option, const PricingContext& context,
                                             const std::array<double, count>& spots) const
    {
        const auto time = detail::time_to_expiry(context.valuation_time(), option.effective_date(), option.expiry_date());
        if (!time) return std::unexpected(time.error());
        std::array<double, count> values{};
        if (*time == 0.0) {
            for (std::size_t i = 0; i < count; ++i) {
                const auto value = detail::checked_price(payoff(option.option_type(), spots[i], option.strike()));
                if (!value) return std::unexpected(value.error());
                values[i] = *value;
            }
            return values;
        }
        auto simulation = settings_;
        simulation.step_count = 2;
        if (simulation.thread_count == 0 && simulation.path_count <= 3072) simulation.thread_count = 1;
        const auto parameters = simulation_parameters(context, *time, simulation);
        if (!parameters) return std::unexpected(parameters.error());
        const bool antithetic = simulation.antithetic;
        const int sample_count = antithetic ? (simulation.path_count + 1) / 2 : simulation.path_count;
        std::vector<std::array<detail::MonteCarloMean, count>> partials(
            static_cast<std::size_t>(detail::monte_carlo_block_count(sample_count)));
        const bool generate = normal_draws_.empty();
        if (generate && cache_samples_) {
            normal_draws_.resize(static_cast<std::size_t>(sample_count));
            growth_.resize(normal_draws_.size());
        }
        const bool update = generate || drift_ != parameters->drift || diffusion_ != parameters->diffusion;
        std::optional<detail::SobolSequence> sobol;
        if (simulation.sampling == MonteCarloSampling::sobol && generate) {
            assert(simulation.seed);
            sobol.emplace(1, *simulation.seed);
        }
        const auto result = detail::monte_carlo_blocks(sample_count, simulation.thread_count,
                                                       [&](int block, int begin, int end) -> Result<void> {
                                                           const auto simulate = [&](auto& generator) -> Result<void> {
                                                               std::normal_distribution<double> normal;
                                                               for (int path = begin; path < end; ++path) {
                                                                   const bool paired = antithetic && path < simulation.path_count / 2;
                                                                   const auto index = static_cast<std::size_t>(path);
                                                                   const double draw = generate ? detail::draw_monte_carlo_normal(generator, normal) : normal_draws_[index];
                                                                   if (generate && cache_samples_) normal_draws_[index] = draw;
                                                                   const auto growth = update ? std::array{std::exp(std::fma(parameters->diffusion, draw, parameters->drift)),
                                                                                                           paired ? std::exp(std::fma(-parameters->diffusion, draw, parameters->drift)) : 1.0}
                                                                                              : growth_[index];
                                                                   if (update && cache_samples_) growth_[index] = growth;
                                                                   for (std::size_t i = 0; i < count; ++i) {
                                                                       const double positive = spots[i] * growth[0];
                                                                       const double negative = spots[i] * growth[1];
                                                                       if (!std::isfinite(positive) || positive <= 0.0 || !std::isfinite(negative) || negative <= 0.0)
                                                                           return std::unexpected(Error{ErrorCategory::invalid_result,
                                                                                                        "Monte Carlo simulation produced a non-finite path"});
                                                                       auto& partial = partials[static_cast<std::size_t>(block)][i];
                                                                       partial.add(payoff(option.option_type(), positive, option.strike()));
                                                                       if (paired) partial.add(payoff(option.option_type(), negative, option.strike()));
                                                                   }
                                                                   if constexpr (std::same_as<std::remove_cvref_t<decltype(generator)>, detail::SobolCursor>) generator.next_path();
                                                               }
                                                               return {};
                                                           };
                                                           if (sobol) {
                                                               detail::SobolCursor generator{*sobol, static_cast<std::uint64_t>(begin)};
                                                               return simulate(generator);
                                                           }
                                                           detail::Pcg32 generator{*simulation.seed, static_cast<std::uint64_t>(block)};
                                                           return simulate(generator);
                                                       });
        if (!result) {
            normal_draws_.clear();
            drift_ = std::numeric_limits<double>::quiet_NaN();
            return std::unexpected(result.error());
        }
        drift_ = parameters->drift;
        diffusion_ = parameters->diffusion;
        std::array<detail::MonteCarloMean, count> means{};
        for (const auto& partial : partials)
            for (std::size_t i = 0; i < count; ++i)
                means[i].merge(partial[i]);
        for (std::size_t i = 0; i < count; ++i) {
            const auto value = discounted_european_mean(means[i].value(), parameters->rate, *time);
            if (!value) return std::unexpected(value.error());
            values[i] = *value;
        }
        return values;
    }

    MonteCarloSettings settings_;
    bool cache_samples_;
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
    if (auto valid = detail::require_flat_rates(context); !valid) return std::unexpected(valid.error());
    std::optional<EuropeanMonteCarloSamples> samples;
    return detail::solve_implied_volatility(engine, option, context, observed_price, settings,
                                            [&](const auto& seeded, const auto& instrument, const auto& shifted) {
                                                if (!samples) samples.emplace(seeded.settings());
                                                return samples->price(instrument, shifted);
                                            });
}

Result<double> MonteCarloVanillaEngine::price(const EuropeanOption& option, const PricingContext& context) const
{
    if (auto valid = detail::require_flat_rates(context); !valid) return std::unexpected(valid.error());
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
    if (*time == 0.0)
        return detail::checked_price(payoff(option.option_type(), context.spot_price(), option.strike()));
    if (settings.step_count < 3)
        return std::unexpected(Error{ErrorCategory::invalid_parameter,
                                     "American Monte Carlo requires at least three grid points"});
    if (terminal_only && option.option_type() == OptionType::call &&
        (*context.model_parameters().dividend_yield()) <= 0.0 &&
        (*context.model_parameters().risk_free_rate()) >= 0.0) {
        const auto european = *make_european_option(
            option.option_type(), option.strike(), option.effective_date(), option.expiry_date());
        const auto value = price_european(european, context, settings, [&](auto parameters, auto simulation, const auto& observe) { return simulate_paths(parameters, simulation, observe, samples); }, workspace);
        if (!value) return std::unexpected(value.error());
        return std::max(*value, payoff(option.option_type(), context.spot_price(), option.strike()));
    }
    const auto parameters = simulation_parameters(context, *time, settings);
    if (!parameters) return std::unexpected(parameters.error());
    const double discount = std::exp(-(*context.model_parameters().risk_free_rate()) *
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
        const auto path_count = static_cast<std::size_t>(settings.path_count);
        const auto step_count = static_cast<std::size_t>(settings.step_count);
        // Only future time slices are stored, contiguously for exercise regression.
        std::vector<double> owned_paths;
        std::vector<double> owned_cash_flows;
        auto& paths = samples ? samples->paths : owned_paths;
        auto& cash_flows = samples ? samples->cash_flows : owned_cash_flows;
        paths.resize(path_count * (step_count - 1));
        const auto simulated = simulate_paths(*parameters, settings, [&](int positive, int negative, int step, double positive_spot, double negative_spot) {
            paths[static_cast<std::size_t>(step - 1) * path_count + static_cast<std::size_t>(positive)] = positive_spot;
            if (negative != positive) paths[static_cast<std::size_t>(step - 1) * path_count + static_cast<std::size_t>(negative)] = negative_spot; }, samples);
        if (!simulated) return std::unexpected(simulated.error());
        cash_flows.resize(path_count);
        const int count = static_cast<int>(path_count);
        // Each worker needs enough paths to amortize scheduling at every exercise date.
        const int exercise_threads = count <= 1024 ? 1 : settings.thread_count == 0 ? std::min(oneapi::tbb::this_task_arena::max_concurrency(), count / 1024)
                                                                                    : settings.thread_count;
        detail::MonteCarloBlocks exercise_blocks{count, exercise_threads};
        const auto initialized = exercise_blocks.run(
            [&](int, int begin, int end) -> Result<void> {
                for (int path = begin; path < end; ++path)
                    cash_flows[path] = payoff(option.option_type(),
                                              paths[(step_count - 2) * path_count + path], option.strike());
                return {};
            });
        if (!initialized) return std::unexpected(initialized.error());
        std::vector<detail::QuadraticRegressionMatrix> matrices(detail::monte_carlo_block_count(count));
        for (int step = settings.step_count - 2; step >= 1; --step) {
            const auto accumulated = exercise_blocks.run(
                [&](int block, int begin, int end) -> Result<void> {
                    detail::QuadraticRegressionMatrix matrix{};
                    for (int path = begin; path < end; ++path)
                        cash_flows[path] *= discount;
                    for (int path = begin; path < end; ++path) {
                        const double spot = paths[static_cast<std::size_t>(step - 1) * path_count + path];
                        if (payoff(option.option_type(), spot, option.strike()) > 0.0) {
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
                    matrices[block] = matrix;
                    return {};
                });
            if (!accumulated) return std::unexpected(accumulated.error());
            // Fixed block order makes regression independent of worker scheduling.
            detail::QuadraticRegressionMatrix matrix{};
            for (const auto& partial : matrices)
                for (std::size_t row = 0; row < matrix.size(); ++row)
                    for (std::size_t column = row; column < matrix[row].size(); ++column)
                        matrix[row][column] += partial[row][column];
            if (matrix[0][0] == 0.0) continue;
            matrix[1][0] = matrix[0][1];
            matrix[2][0] = matrix[0][2];
            matrix[2][1] = matrix[1][2];
            std::array<double, 3> coefficients{};
            if (!detail::solve_quadratic(matrix, coefficients)) continue;
            const auto exercised = exercise_blocks.run(
                [&](int, int begin, int end) -> Result<void> {
                    for (int path = begin; path < end; ++path) {
                        const double spot = paths[static_cast<std::size_t>(step - 1) * path_count + path];
                        const double intrinsic = payoff(option.option_type(), spot, option.strike());
                        if (intrinsic <= 0.0) continue;
                        const double scaled = spot / option.strike();
                        const double continuation =
                            coefficients[0] + scaled * (coefficients[1] + scaled * coefficients[2]);
                        if (std::isfinite(continuation) && intrinsic / option.strike() > continuation)
                            cash_flows[path] = intrinsic;
                    }
                    return {};
                });
            if (!exercised) return std::unexpected(exercised.error());
        }
        std::vector<detail::MonteCarloMean> block_payoffs(matrices.size());
        const auto reduced = exercise_blocks.run(
            [&](int block, int begin, int end) -> Result<void> {
                detail::MonteCarloMean partial{};
                for (int path = begin; path < end; ++path)
                    partial.add(cash_flows[path]);
                block_payoffs[block] = partial;
                return {};
            });
        if (!reduced) return std::unexpected(reduced.error());
        detail::MonteCarloMean payoffs{};
        for (const auto& partial : block_payoffs)
            payoffs.merge(partial);
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
    explicit AmericanMonteCarloPricer(MonteCarloSettings settings, bool cache_growth = false, bool cache_normals = false)
        : settings_(settings), samples_{.cache_growth = cache_growth, .cache_normals = cache_normals} {}

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
    if (auto valid = detail::require_flat_rates(context); !valid) return std::unexpected(valid.error());
    std::optional<AmericanMonteCarloPricer> prices;
    return detail::solve_implied_volatility(engine, option, context, observed_price, settings,
                                            [&](const auto& seeded, const auto& instrument, const auto& shifted) {
                                                if (!prices) prices.emplace(seeded.settings(), false, true);
                                                return prices->price(instrument, shifted);
                                            });
}

Result<double> MonteCarloVanillaEngine::price(const AmericanOption& option, const PricingContext& context) const
{
    if (auto valid = detail::require_flat_rates(context); !valid) return std::unexpected(valid.error());
    return price_american(option, context, settings_, true);
}

Result<PricingResult> MonteCarloVanillaEngine::price_with_greeks(const EuropeanOption& option, const PricingContext& context,
                                                                 GreeksRequest greeks, NumericalShiftSettings settings) const
{
    if (auto valid = detail::require_flat_rates(context); !valid) return std::unexpected(valid.error());
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
        const bool reuse_samples = greeks.has(Greek::theta) || greeks.has(Greek::charm) || greeks.has(Greek::color) ||
                                   greeks.has(Greek::vega) || greeks.has(Greek::vanna) || greeks.has(Greek::zomma) || greeks.has(Greek::rho);
        const EuropeanMonteCarloSamples samples{seeded.settings(), reuse_samples};
        const auto price = [&](const PricingContext& shifted) { return samples.price(option, shifted); };
        const auto native = [&]() -> Result<PricingResult> {
            if (seeded.settings().backend == MonteCarloBackend::cpu && stencils.spot_available &&
                !detail::greeks_unavailable(option, context) &&
                (greeks.has(Greek::delta) || greeks.has(Greek::gamma) || (greeks.has(Greek::speed) && stencils.speed_available))) {
                if (greeks.has(Greek::speed) && stencils.speed_available)
                    return detail::spot_bump_result(samples.spot_prices<5>(option, context, stencils.spot_shift), greeks, stencils.spot_shift);
                return detail::spot_bump_result(samples.spot_prices<3>(option, context, stencils.spot_shift), greeks, stencils.spot_shift);
            }
            const auto value = price(context);
            if (!value) return std::unexpected(value.error());
            return make_pricing_result(*value);
        }();
        if (!native) return std::unexpected(native.error());
        if (seeded.settings().backend == MonteCarloBackend::cpu) {
            const auto spot_batch = [&](const PricingContext& shifted, double h, bool center) -> Result<detail::NumericalSpotPrices> {
                const auto sample = [&]<std::size_t count>() -> Result<detail::NumericalSpotPrices> {
                    const auto values = samples.spot_prices<count>(option, shifted, h);
                    if (!values) return std::unexpected(values.error());
                    const auto middle = [&]() -> std::optional<double> {
                        if constexpr (count == 3) return (*values)[detail::SpotStencil<count>::center];
                        return std::nullopt;
                    }();
                    return detail::NumericalSpotPrices{middle, (*values)[detail::SpotStencil<count>::up], (*values)[detail::SpotStencil<count>::down]};
                };
                return center ? sample.template operator()<3>() : sample.template operator()<2>();
            };
            return detail::complete_greeks(price, option, context, greeks, settings, *native, nullptr, spot_batch);
        }
        return detail::complete_greeks(price, option, context, greeks, settings, *native); });
}

Result<PricingResult> MonteCarloVanillaEngine::price_with_greeks(const AmericanOption& option, const PricingContext& context,
                                                                 GreeksRequest greeks, NumericalShiftSettings settings) const
{
    if (auto valid = detail::require_flat_rates(context); !valid) return std::unexpected(valid.error());
    return detail::evaluate_greeks(*this, option, context, greeks, settings, [&](const auto& seeded) -> Result<PricingResult> {
        const auto stencils = detail::numerical_stencils(option, context, settings);
        const bool cache_growth = stencils.spot_available &&
            (greeks.has(Greek::delta) || greeks.has(Greek::gamma) || (greeks.has(Greek::speed) && stencils.speed_available) ||
             (stencils.volatility_available && (greeks.has(Greek::vanna) || greeks.has(Greek::zomma))) ||
             greeks.has(Greek::charm) || greeks.has(Greek::color));
        const bool cache_normals = seeded.settings().backend == MonteCarloBackend::cpu &&
            ((stencils.volatility_available && greeks.has(Greek::vega)) ||
             (stencils.rate_available && greeks.has(Greek::rho)));
        const AmericanMonteCarloPricer prices{seeded.settings(), cache_growth, cache_normals};
        const auto native = [&]() -> Result<PricingResult> {
        const auto value = prices.price(option, context);
        if (!value) return std::unexpected(value.error());
        const double rate = (*context.model_parameters().risk_free_rate());
        const double shift = stencils.rate_shift;
        if (greeks.has(Greek::rho) && stencils.rate_available && !detail::greeks_unavailable(option, context) &&
            option.option_type() == OptionType::call && (*context.model_parameters().dividend_yield()) <= 0.0 &&
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
