#include <array>
#include <chrono>
#include <cmath>
#include <format>
#include <string>

#include <kiyosi/kiyosi.hpp>

#include "pricing.hpp"

namespace kiyosi::benchmark_support {
namespace {
template <typename Option>
void add_asian_case(const std::string& name, const Option& option, const PricingContext& context,
                    const MonteCarloAsianEngine& engine, double reference, double reference_error)
{
    for (const bool greeks : {false, true}) {
        benchmark::RegisterBenchmark(name + (greeks ? "/delta_gamma" : "/price"),
                                     [=](benchmark::State& state) {
                                         const auto evaluate = [&]() -> Result<double> {
                                             if (!greeks) return engine.price(option, context);
                                             const auto result = engine.price_with_greeks(option, context, {Greek::delta, Greek::gamma});
                                             if (!result) return std::unexpected(result.error());
                                             return result->price();
                                         };
                                         const auto warmup = evaluate();
                                         if (!warmup) {
                                             state.SkipWithError(warmup.error().message);
                                             return;
                                         }
                                         state.counters["price"] = *warmup;
                                         state.counters["reference"] = reference;
                                         state.counters["absolute_error"] = std::abs(*warmup - reference);
                                         state.counters["reference_standard_error"] = reference_error;
                                         for (auto _ : state) {
                                             auto result = evaluate();
                                             if (!result) {
                                                 state.SkipWithError(result.error().message);
                                                 break;
                                             }
                                             benchmark::DoNotOptimize(result);
                                         }
                                     })
            ->UseRealTime();
    }
}
} // namespace

void register_asian_cases()
{
    const Date start{std::chrono::year{2025} / 1 / 1};
    const auto expiry = start + std::chrono::days{365};
    // Ordinary: published Vecer continuous price (QuantLib testVecerEngine).
    // Other windows: independent raw-path antithetic MT19937-64 simulation, 4,194,304
    // pairs, 16 streams seeded 9917..9932, 512 intervals; 128-interval estimates agree
    // within sampling uncertainty. The ordinary published value has tolerance 1e-5.
    // These errors include reference sampling uncertainty; they are not certified error bounds.
    const std::array references{0.246416, 0.318441244391, 0.0878839747412}; // NOLINT(modernize-use-std-numbers): independently simulated reference prices.
    const std::array reference_errors{0.0, 0.00015912013, 0.000039326393};
    for (int window = 0; window < 3; ++window) {
        const auto averaging = window == 1 ? start + std::chrono::days{120} : start;
        const auto valuation = window == 2 ? start + std::chrono::days{180} : start;
        const auto context = *make_pricing_context(*make_bsm_parameters(0.05, 0.0, 0.5), 2.0, valuation);
        const double history = window == 2 ? 2.0 : 0.0;
        const auto arithmetic = *make_arithmetic_average_option(OptionType::call, 2.0, start, averaging, expiry, history);
        const auto geometric = *make_geometric_average_option(OptionType::call, 2.0, start, averaging, expiry, history);
        const double geometric_reference = AnalyticGeometricAveragePriceEngine{}.price(geometric, context).value();
        const std::string scenario = window == 0 ? "ordinary/" : window == 1 ? "forward_start/"
                                                                             : "seasoned/";
        for (const auto backend : {MonteCarloBackend::cpu, MonteCarloBackend::cuda}) {
#if !KIYOSI_HAS_CUDA
            if (backend == MonteCarloBackend::cuda) continue;
#endif
            for (const auto sampling : {MonteCarloSampling::pseudo_random, MonteCarloSampling::sobol}) {
                if (backend == MonteCarloBackend::cuda && sampling == MonteCarloSampling::sobol) continue;
                const std::string mode = backend == MonteCarloBackend::cuda ? "cuda/pseudo_random" : sampling == MonteCarloSampling::sobol ? "cpu/sobol"
                                                                                                                                           : "cpu/pseudo_random";
                for (const int steps : {16, 64, 256}) {
                    const auto engine = make_monte_carlo_asian_engine({.path_count = 65'536, .step_count = steps, .seed = 42, .backend = backend, .sampling = sampling, .antithetic = true}).value();
                    add_asian_case(std::format("asian/arithmetic/{}{}/{}", scenario, mode, steps), arithmetic, context, engine,
                                   references[window], reference_errors[window]);
                    if (steps == 16) add_asian_case(std::format("asian/geometric/{}{}", scenario, mode), geometric, context, engine, geometric_reference, 0.0);
                }
            }
        }
    }
}
} // namespace kiyosi::benchmark_support
