#include <chrono>

#include <benchmark/benchmark.h>
#include <kiyosi/kiyosi.hpp>

namespace {

struct Scenario {
    kiyosi::EuropeanOption option;
    kiyosi::PricingContext context;
};

const Scenario& monte_carlo_scenario()
{
    static const auto value = [] {
        const kiyosi::date effective{std::chrono::year{2025} / 1 / 1};
        const kiyosi::date expiry{std::chrono::year{2026} / 1 / 1};
        return Scenario{
            *kiyosi::make_european_option(
                kiyosi::option_type::call, 100.0, effective, expiry),
            *kiyosi::make_pricing_context(
                *kiyosi::make_bsm_parameters(0.04, 0.01, 0.2), 100.0, effective)};
    }();
    return value;
}

void benchmark_monte_carlo(benchmark::State& state, kiyosi::monte_carlo_backend backend)
{
    const auto& [option, context] = monte_carlo_scenario();
    const kiyosi::MonteCarloVanillaEngine engine{
        kiyosi::MonteCarloSettings{1'000'000, 50, 42, backend}};
    const auto warmup = engine.price(option, context);
    if (!warmup) {
        state.SkipWithError(warmup.error().message.c_str());
        return;
    }
    for (auto _ : state) {
        auto result = engine.price(option, context);
        benchmark::DoNotOptimize(result);
    }
}

void BM_MonteCarloEuropeanCpu(benchmark::State& state)
{
    benchmark_monte_carlo(state, kiyosi::monte_carlo_backend::cpu);
}

#if KIYOSI_HAS_CUDA
void BM_MonteCarloEuropeanCuda(benchmark::State& state)
{
    benchmark_monte_carlo(state, kiyosi::monte_carlo_backend::cuda);
}
#endif

} // namespace

BENCHMARK(BM_MonteCarloEuropeanCpu)
    ->UseRealTime()
    ->Repetitions(5)
    ->ReportAggregatesOnly(true)
    ->Unit(benchmark::kMillisecond);

#if KIYOSI_HAS_CUDA
BENCHMARK(BM_MonteCarloEuropeanCuda)
    ->UseRealTime()
    ->Repetitions(5)
    ->ReportAggregatesOnly(true)
    ->Unit(benchmark::kMillisecond);
#endif
