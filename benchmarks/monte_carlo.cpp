#include <chrono>
#include <vector>

#include <benchmark/benchmark.h>
#include <kiyosi/kiyosi.hpp>

namespace {

struct Scenario {
    kiyosi::EuropeanOption option;
    kiyosi::PricingContext context;
};

struct AmericanScenario {
    kiyosi::AmericanOption option;
    kiyosi::PricingContext context;
};

struct AccumulatorScenario {
    kiyosi::Accumulator option;
    kiyosi::PricingContext context;
};

template <typename Note>
struct StructuredScenario {
    Note note;
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

const AmericanScenario& american_monte_carlo_scenario()
{
    static const auto value = [] {
        const kiyosi::date effective{std::chrono::year{2025} / 1 / 1};
        const kiyosi::date expiry{std::chrono::year{2026} / 1 / 1};
        return AmericanScenario{
            *kiyosi::make_american_option(
                kiyosi::option_type::put, 100.0, effective, expiry),
            *kiyosi::make_pricing_context(
                *kiyosi::make_bsm_parameters(0.05, 0.02, 0.2), 100.0, effective)};
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

void benchmark_american_monte_carlo(
    benchmark::State& state, kiyosi::monte_carlo_backend backend)
{
    const auto& [option, context] = american_monte_carlo_scenario();
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

void BM_MonteCarloAmericanCpu(benchmark::State& state)
{
    benchmark_american_monte_carlo(state, kiyosi::monte_carlo_backend::cpu);
}

const AccumulatorScenario& accumulator_scenario()
{
    static const auto value = [] {
        const kiyosi::date effective{std::chrono::year{2025} / 1 / 1};
        const kiyosi::date expiry{std::chrono::year{2026} / 1 / 1};
        return AccumulatorScenario{
            *kiyosi::make_accumulator({.strike = 100.0,
                                       .knock_out = 115.0,
                                       .daily_quantity = 1.0,
                                       .acceleration = 2.0,
                                       .accumulated_quantity = 0.0,
                                       .effective = effective,
                                       .expiry = expiry}),
            *kiyosi::make_pricing_context(
                *kiyosi::make_bsm_parameters(0.04, 0.01, 0.2), 100.0, effective)};
    }();
    return value;
}

const StructuredScenario<kiyosi::PhoenixOption>& phoenix_scenario()
{
    static const auto value = [] {
        const kiyosi::date effective{std::chrono::year{2025} / 1 / 1};
        const kiyosi::date expiry{std::chrono::year{2026} / 1 / 1};
        const std::vector<kiyosi::date> observations{
            kiyosi::date{std::chrono::year{2025} / 4 / 1},
            kiyosi::date{std::chrono::year{2025} / 7 / 1},
            kiyosi::date{std::chrono::year{2025} / 10 / 1}, expiry};
        return StructuredScenario<kiyosi::PhoenixOption>{
            *kiyosi::make_phoenix_option({.coupon_rate = 0.002,
                                          .initial_price = 100.0,
                                          .knock_in_price = 75.0,
                                          .knock_out_prices = {110.0, 108.0, 106.0, 104.0},
                                          .coupon_barriers = {90.0, 90.0, 90.0, 90.0},
                                          .upper_strike = 100.0,
                                          .lower_strike = 60.0,
                                          .observation_dates = observations,
                                          .frequency = kiyosi::observation_frequency::daily,
                                          .touch_status = kiyosi::barrier_touch_status::none,
                                          .principal_ratio = 1.0,
                                          .effective = effective,
                                          .expiry = expiry}),
            *kiyosi::make_pricing_context(
                *kiyosi::make_bsm_parameters(0.04, 0.01, 0.2), 100.0, effective)};
    }();
    return value;
}

const StructuredScenario<kiyosi::SnowballOption>& snowball_scenario()
{
    static const auto value = [] {
        const kiyosi::date effective{std::chrono::year{2025} / 1 / 1};
        const kiyosi::date expiry{std::chrono::year{2026} / 1 / 1};
        const std::vector<kiyosi::date> observations{
            kiyosi::date{std::chrono::year{2025} / 4 / 1},
            kiyosi::date{std::chrono::year{2025} / 7 / 1},
            kiyosi::date{std::chrono::year{2025} / 10 / 1}, expiry};
        return StructuredScenario<kiyosi::SnowballOption>{
            *kiyosi::make_snowball_option({.knock_out_coupon_rates = {0.08, 0.08, 0.08, 0.08},
                                           .maturity_coupon_rate = 0.06,
                                           .initial_price = 100.0,
                                           .knock_in_price = 75.0,
                                           .knock_out_prices = {110.0, 108.0, 106.0, 104.0},
                                           .upper_strike = 100.0,
                                           .lower_strike = 60.0,
                                           .observation_dates = observations,
                                           .frequency = kiyosi::observation_frequency::daily,
                                           .touch_status = kiyosi::barrier_touch_status::none,
                                           .principal_ratio = 1.0,
                                           .effective = effective,
                                           .expiry = expiry}),
            *kiyosi::make_pricing_context(
                *kiyosi::make_bsm_parameters(0.04, 0.01, 0.2), 100.0, effective)};
    }();
    return value;
}

void benchmark_accumulator_monte_carlo(
    benchmark::State& state, kiyosi::monte_carlo_backend backend)
{
    const auto& [option, context] = accumulator_scenario();
    const kiyosi::MonteCarloAccumulatorEngine engine{{250'000, 42, backend}};
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

template <typename Note, typename Engine>
void benchmark_structured_monte_carlo(
    benchmark::State& state, const StructuredScenario<Note>& scenario,
    kiyosi::monte_carlo_backend backend)
{
    const Engine engine{{250'000, 42, backend}};
    const auto warmup = engine.price(scenario.note, scenario.context);
    if (!warmup) {
        state.SkipWithError(warmup.error().message.c_str());
        return;
    }
    for (auto _ : state) {
        auto result = engine.price(scenario.note, scenario.context);
        benchmark::DoNotOptimize(result);
    }
}

void BM_MonteCarloAccumulatorCpu(benchmark::State& state)
{
    benchmark_accumulator_monte_carlo(state, kiyosi::monte_carlo_backend::cpu);
}

void BM_MonteCarloPhoenixCpu(benchmark::State& state)
{
    benchmark_structured_monte_carlo<kiyosi::PhoenixOption, kiyosi::MonteCarloPhoenixEngine>(
        state, phoenix_scenario(), kiyosi::monte_carlo_backend::cpu);
}

void BM_MonteCarloSnowballCpu(benchmark::State& state)
{
    benchmark_structured_monte_carlo<kiyosi::SnowballOption, kiyosi::MonteCarloSnowballEngine>(
        state, snowball_scenario(), kiyosi::monte_carlo_backend::cpu);
}

#if KIYOSI_HAS_CUDA
void BM_MonteCarloEuropeanCuda(benchmark::State& state)
{
    benchmark_monte_carlo(state, kiyosi::monte_carlo_backend::cuda);
}

void BM_MonteCarloAmericanCuda(benchmark::State& state)
{
    benchmark_american_monte_carlo(state, kiyosi::monte_carlo_backend::cuda);
}

void BM_MonteCarloAccumulatorCuda(benchmark::State& state)
{
    benchmark_accumulator_monte_carlo(state, kiyosi::monte_carlo_backend::cuda);
}

void BM_MonteCarloPhoenixCuda(benchmark::State& state)
{
    benchmark_structured_monte_carlo<kiyosi::PhoenixOption, kiyosi::MonteCarloPhoenixEngine>(
        state, phoenix_scenario(), kiyosi::monte_carlo_backend::cuda);
}

void BM_MonteCarloSnowballCuda(benchmark::State& state)
{
    benchmark_structured_monte_carlo<kiyosi::SnowballOption, kiyosi::MonteCarloSnowballEngine>(
        state, snowball_scenario(), kiyosi::monte_carlo_backend::cuda);
}
#endif

} // namespace

BENCHMARK(BM_MonteCarloEuropeanCpu)
    ->UseRealTime()
    ->Repetitions(5)
    ->ReportAggregatesOnly(true)
    ->Unit(benchmark::kMillisecond);

BENCHMARK(BM_MonteCarloAmericanCpu)
    ->UseRealTime()
    ->Repetitions(5)
    ->ReportAggregatesOnly(true)
    ->Unit(benchmark::kMillisecond);

BENCHMARK(BM_MonteCarloAccumulatorCpu)
    ->UseRealTime()
    ->Repetitions(5)
    ->ReportAggregatesOnly(true)
    ->Unit(benchmark::kMillisecond);

BENCHMARK(BM_MonteCarloPhoenixCpu)
    ->UseRealTime()
    ->Repetitions(5)
    ->ReportAggregatesOnly(true)
    ->Unit(benchmark::kMillisecond);

BENCHMARK(BM_MonteCarloSnowballCpu)
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

BENCHMARK(BM_MonteCarloAmericanCuda)
    ->UseRealTime()
    ->Repetitions(5)
    ->ReportAggregatesOnly(true)
    ->Unit(benchmark::kMillisecond);

BENCHMARK(BM_MonteCarloAccumulatorCuda)
    ->UseRealTime()
    ->Repetitions(5)
    ->ReportAggregatesOnly(true)
    ->Unit(benchmark::kMillisecond);

BENCHMARK(BM_MonteCarloPhoenixCuda)
    ->UseRealTime()
    ->Repetitions(5)
    ->ReportAggregatesOnly(true)
    ->Unit(benchmark::kMillisecond);

BENCHMARK(BM_MonteCarloSnowballCuda)
    ->UseRealTime()
    ->Repetitions(5)
    ->ReportAggregatesOnly(true)
    ->Unit(benchmark::kMillisecond);
#endif
