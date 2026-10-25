#include <chrono>
#include <cmath>
#include <string>
#include <vector>

#include <kiyosi/kiyosi.hpp>

#include "pricing.hpp"

namespace kiyosi::benchmark_support {
namespace {

template <typename Engine, typename Option>
void add_barrier_case(const std::string& name, const Option& option,
                      const PricingContext& context, const Engine& engine, double reference)
{
    const auto add = [&](bool greeks) {
        benchmark::RegisterBenchmark(
            name + (greeks ? "/delta_gamma" : "/price"),
            [greeks, reference, option = option, context = context, engine = engine](benchmark::State& state) {
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
                for (auto _ : state) {
                    auto value = evaluate();
                    if (!value) {
                        state.SkipWithError(value.error().message);
                        break;
                    }
                    benchmark::DoNotOptimize(value);
                }
            })
            ->UseRealTime();
    };
    add(false);
    add(true);
}

} // namespace

void register_barrier_cases()
{
    const Date start{std::chrono::year{2025} / 1 / 1};
    const auto expiry = start + std::chrono::days{360};
    const auto context = *make_pricing_context(*make_bsm_parameters(0.03, 0.01, 0.2), 100.0, start, all_days_calendar());
    for (const bool scheduled : {false, true}) {
        const auto option = *make_barrier_option({.option_type = OptionType::call, .strike = 100.0, .effective_date = start, .expiry_date = expiry, .barrier_level = 80.0, .barrier_type = BarrierType::down_and_out, .rebate = 2.0, .observation_mode = scheduled ? ObservationMode::scheduled : ObservationMode::continuous, .observation_dates = scheduled ? std::vector<Date>{start + std::chrono::days{90}, start + std::chrono::days{180}, expiry} : std::vector<Date>{}});
        const auto reference = scheduled
                                   ? make_finite_difference_barrier_engine({2000, 2000})->price(option, context)
                                   : AnalyticBarrierEngine{}.price(option, context);
        if (!reference) continue;
        const std::string prefix = std::string{"barrier/"} + (scheduled ? "scheduled/" : "continuous/");
        for (const int steps : {360, 1440})
            add_barrier_case(prefix + "crr/" + std::to_string(steps), option, context,
                             *make_cox_ross_rubinstein_barrier_engine({steps}), *reference);
        for (const int steps : {256, 512})
            add_barrier_case(prefix + "quadrature/" + std::to_string(steps), option, context,
                             *make_quadrature_barrier_engine({steps}), *reference);
        for (const auto sampling : {MonteCarloSampling::pseudo_random, MonteCarloSampling::sobol})
            add_barrier_case(prefix + "mc/cpu/" + (sampling == MonteCarloSampling::sobol ? "sobol" : "pseudo_random"),
                             option, context, *make_monte_carlo_barrier_engine({.path_count = 65'536, .seed = 42, .sampling = sampling}), *reference);
#if KIYOSI_HAS_CUDA
        add_barrier_case(prefix + "mc/cuda", option, context,
                         *make_monte_carlo_barrier_engine({.path_count = 65'536, .seed = 42, .backend = MonteCarloBackend::cuda}), *reference);
#endif
    }
    for (const bool narrow : {false, true}) {
        for (const bool scheduled : {false, true}) {
            const auto option = *make_double_barrier_option({.option_type = OptionType::call, .strike = 100.0, .effective_date = start, .expiry_date = expiry, .lower_barrier = narrow ? 95.0 : 80.0, .upper_barrier = narrow ? 105.0 : 120.0, .barrier_type = DoubleBarrierType::knock_out, .rebate = 2.0, .observation_mode = scheduled ? ObservationMode::scheduled : ObservationMode::continuous, .observation_dates = scheduled ? std::vector<Date>{start + std::chrono::days{90}, start + std::chrono::days{180}, expiry} : std::vector<Date>{}});
            const auto reference = scheduled ? make_finite_difference_double_barrier_engine({2000, 2000})->price(option, context)
                                             : AnalyticDoubleBarrierEngine{}.price(option, context);
            if (!reference) continue;
            const std::string prefix = std::string{"double_barrier/"} + (narrow ? "narrow/" : "ordinary/") + (scheduled ? "scheduled/" : "continuous/");
            if (!scheduled) add_barrier_case(prefix + "analytic", option, context, AnalyticDoubleBarrierEngine{}, *reference);
            for (const auto sampling : {MonteCarloSampling::pseudo_random, MonteCarloSampling::sobol})
                add_barrier_case(prefix + "mc/cpu/" + (sampling == MonteCarloSampling::sobol ? "sobol" : "pseudo_random"), option, context,
                                 *make_monte_carlo_double_barrier_engine({.path_count = 65'536, .seed = 42, .sampling = sampling}), *reference);
#if KIYOSI_HAS_CUDA
            add_barrier_case(prefix + "mc/cuda", option, context,
                             *make_monte_carlo_double_barrier_engine({.path_count = 65'536, .seed = 42, .backend = MonteCarloBackend::cuda}), *reference);
#endif
        }
    }
}

} // namespace kiyosi::benchmark_support
