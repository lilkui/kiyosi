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
        const kiyosi::Date effective_date{std::chrono::year{2025} / 1 / 1};
        const kiyosi::Date expiry_date{std::chrono::year{2026} / 1 / 1};
        return Scenario{
            *kiyosi::make_european_option(
                kiyosi::OptionType::call, 100.0, effective_date, expiry_date),
            *kiyosi::make_pricing_context(
                *kiyosi::make_bsm_parameters(0.04, 0.01, 0.2), 100.0, effective_date)};
    }();
    return value;
}

const AmericanScenario& american_monte_carlo_scenario()
{
    static const auto value = [] {
        const kiyosi::Date effective_date{std::chrono::year{2025} / 1 / 1};
        const kiyosi::Date expiry_date{std::chrono::year{2026} / 1 / 1};
        return AmericanScenario{
            *kiyosi::make_american_option(
                kiyosi::OptionType::put, 100.0, effective_date, expiry_date),
            *kiyosi::make_pricing_context(
                *kiyosi::make_bsm_parameters(0.05, 0.02, 0.2), 100.0, effective_date)};
    }();
    return value;
}

void benchmark_monte_carlo(benchmark::State& state, kiyosi::MonteCarloBackend backend)
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

void bm_monte_carlo_european_cpu(benchmark::State& state)
{
    benchmark_monte_carlo(state, kiyosi::MonteCarloBackend::cpu);
}

void benchmark_american_monte_carlo(
    benchmark::State& state, kiyosi::MonteCarloBackend backend)
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

void bm_monte_carlo_american_cpu(benchmark::State& state)
{
    benchmark_american_monte_carlo(state, kiyosi::MonteCarloBackend::cpu);
}

const AccumulatorScenario& accumulator_scenario()
{
    static const auto value = [] {
        const kiyosi::Date effective_date{std::chrono::year{2025} / 1 / 1};
        const kiyosi::Date expiry_date{std::chrono::year{2026} / 1 / 1};
        return AccumulatorScenario{
            *kiyosi::make_accumulator({.strike = 100.0,
                                       .knock_out_level = 115.0,
                                       .daily_quantity = 1.0,
                                       .acceleration_factor = 2.0,
                                       .accumulated_quantity = 0.0,
                                       .effective_date = effective_date,
                                       .expiry_date = expiry_date}),
            *kiyosi::make_pricing_context(
                *kiyosi::make_bsm_parameters(0.04, 0.01, 0.2), 100.0, effective_date)};
    }();
    return value;
}

const StructuredScenario<kiyosi::PhoenixOption>& phoenix_scenario()
{
    static const auto value = [] {
        const kiyosi::Date effective_date{std::chrono::year{2025} / 1 / 1};
        const kiyosi::Date expiry_date{std::chrono::year{2026} / 1 / 1};
        const std::vector<kiyosi::Date> observations{
            kiyosi::Date{std::chrono::year{2025} / 4 / 1},
            kiyosi::Date{std::chrono::year{2025} / 7 / 1},
            kiyosi::Date{std::chrono::year{2025} / 10 / 1}, expiry_date};
        return StructuredScenario<kiyosi::PhoenixOption>{
            *kiyosi::make_phoenix_option({.coupon_rate = 0.002,
                                          .initial_spot = 100.0,
                                          .knock_in_level = 75.0,
                                          .knock_out_levels = {110.0, 108.0, 106.0, 104.0},
                                          .coupon_barrier_levels = {90.0, 90.0, 90.0, 90.0},
                                          .upper_strike = 100.0,
                                          .lower_strike = 60.0,
                                          .observation_dates = observations,
                                          .knock_in_observation_mode = kiyosi::KnockInObservationMode::every_trading_day,
                                          .barrier_state = kiyosi::AutocallableBarrierState::none,
                                          .principal_ratio = 1.0,
                                          .effective_date = effective_date,
                                          .expiry_date = expiry_date}),
            *kiyosi::make_pricing_context(
                *kiyosi::make_bsm_parameters(0.04, 0.01, 0.2), 100.0, effective_date)};
    }();
    return value;
}

const StructuredScenario<kiyosi::SnowballOption>& snowball_scenario()
{
    static const auto value = [] {
        const kiyosi::Date effective_date{std::chrono::year{2025} / 1 / 1};
        const kiyosi::Date expiry_date{std::chrono::year{2026} / 1 / 1};
        const std::vector<kiyosi::Date> observations{
            kiyosi::Date{std::chrono::year{2025} / 4 / 1},
            kiyosi::Date{std::chrono::year{2025} / 7 / 1},
            kiyosi::Date{std::chrono::year{2025} / 10 / 1}, expiry_date};
        return StructuredScenario<kiyosi::SnowballOption>{
            *kiyosi::make_snowball_option({.knock_out_coupon_rates = {0.08, 0.08, 0.08, 0.08},
                                           .maturity_coupon_rate = 0.06,
                                           .initial_spot = 100.0,
                                           .knock_in_level = 75.0,
                                           .knock_out_levels = {110.0, 108.0, 106.0, 104.0},
                                           .upper_strike = 100.0,
                                           .lower_strike = 60.0,
                                           .observation_dates = observations,
                                           .knock_in_observation_mode = kiyosi::KnockInObservationMode::every_trading_day,
                                           .barrier_state = kiyosi::AutocallableBarrierState::none,
                                           .principal_ratio = 1.0,
                                           .effective_date = effective_date,
                                           .expiry_date = expiry_date}),
            *kiyosi::make_pricing_context(
                *kiyosi::make_bsm_parameters(0.04, 0.01, 0.2), 100.0, effective_date)};
    }();
    return value;
}

void benchmark_accumulator_monte_carlo(
    benchmark::State& state, kiyosi::MonteCarloBackend backend)
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
    kiyosi::MonteCarloBackend backend)
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

void bm_monte_carlo_accumulator_cpu(benchmark::State& state)
{
    benchmark_accumulator_monte_carlo(state, kiyosi::MonteCarloBackend::cpu);
}

void bm_monte_carlo_phoenix_cpu(benchmark::State& state)
{
    benchmark_structured_monte_carlo<kiyosi::PhoenixOption, kiyosi::MonteCarloPhoenixEngine>(
        state, phoenix_scenario(), kiyosi::MonteCarloBackend::cpu);
}

void bm_monte_carlo_snowball_cpu(benchmark::State& state)
{
    benchmark_structured_monte_carlo<kiyosi::SnowballOption, kiyosi::MonteCarloSnowballEngine>(
        state, snowball_scenario(), kiyosi::MonteCarloBackend::cpu);
}

#if KIYOSI_HAS_CUDA
void bm_monte_carlo_european_cuda(benchmark::State& state)
{
    benchmark_monte_carlo(state, kiyosi::MonteCarloBackend::cuda);
}

void bm_monte_carlo_american_cuda(benchmark::State& state)
{
    benchmark_american_monte_carlo(state, kiyosi::MonteCarloBackend::cuda);
}

void bm_monte_carlo_accumulator_cuda(benchmark::State& state)
{
    benchmark_accumulator_monte_carlo(state, kiyosi::MonteCarloBackend::cuda);
}

void bm_monte_carlo_phoenix_cuda(benchmark::State& state)
{
    benchmark_structured_monte_carlo<kiyosi::PhoenixOption, kiyosi::MonteCarloPhoenixEngine>(
        state, phoenix_scenario(), kiyosi::MonteCarloBackend::cuda);
}

void bm_monte_carlo_snowball_cuda(benchmark::State& state)
{
    benchmark_structured_monte_carlo<kiyosi::SnowballOption, kiyosi::MonteCarloSnowballEngine>(
        state, snowball_scenario(), kiyosi::MonteCarloBackend::cuda);
}
#endif

} // namespace

BENCHMARK(bm_monte_carlo_european_cpu)
    ->UseRealTime()
    ->Repetitions(5)
    ->ReportAggregatesOnly(true)
    ->Unit(benchmark::kMillisecond);

BENCHMARK(bm_monte_carlo_american_cpu)
    ->UseRealTime()
    ->Repetitions(5)
    ->ReportAggregatesOnly(true)
    ->Unit(benchmark::kMillisecond);

BENCHMARK(bm_monte_carlo_accumulator_cpu)
    ->UseRealTime()
    ->Repetitions(5)
    ->ReportAggregatesOnly(true)
    ->Unit(benchmark::kMillisecond);

BENCHMARK(bm_monte_carlo_phoenix_cpu)
    ->UseRealTime()
    ->Repetitions(5)
    ->ReportAggregatesOnly(true)
    ->Unit(benchmark::kMillisecond);

BENCHMARK(bm_monte_carlo_snowball_cpu)
    ->UseRealTime()
    ->Repetitions(5)
    ->ReportAggregatesOnly(true)
    ->Unit(benchmark::kMillisecond);

#if KIYOSI_HAS_CUDA
BENCHMARK(bm_monte_carlo_european_cuda)
    ->UseRealTime()
    ->Repetitions(5)
    ->ReportAggregatesOnly(true)
    ->Unit(benchmark::kMillisecond);

BENCHMARK(bm_monte_carlo_american_cuda)
    ->UseRealTime()
    ->Repetitions(5)
    ->ReportAggregatesOnly(true)
    ->Unit(benchmark::kMillisecond);

BENCHMARK(bm_monte_carlo_accumulator_cuda)
    ->UseRealTime()
    ->Repetitions(5)
    ->ReportAggregatesOnly(true)
    ->Unit(benchmark::kMillisecond);

BENCHMARK(bm_monte_carlo_phoenix_cuda)
    ->UseRealTime()
    ->Repetitions(5)
    ->ReportAggregatesOnly(true)
    ->Unit(benchmark::kMillisecond);

BENCHMARK(bm_monte_carlo_snowball_cuda)
    ->UseRealTime()
    ->Repetitions(5)
    ->ReportAggregatesOnly(true)
    ->Unit(benchmark::kMillisecond);
#endif
