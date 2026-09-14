#include <chrono>

#include <benchmark/benchmark.h>
#include <kiyosi/kiyosi.hpp>

namespace {

struct Scenario {
    kiyosi::EuropeanOption option;
    kiyosi::PricingContext context;
};

const Scenario& scenario()
{
    static const auto value = [] {
        const kiyosi::date effective{std::chrono::year{2025} / 1 / 1};
        const kiyosi::date expiry{std::chrono::year{2026} / 1 / 1};
        return Scenario{
            *kiyosi::make_european_option(kiyosi::option_type::call, 100.0, effective, expiry),
            *kiyosi::make_pricing_context(
                *kiyosi::make_bsm_parameters(0.04, 0.01, 0.2), 100.0, effective)};
    }();
    return value;
}

void BM_AnalyticEuropeanPrice(benchmark::State& state)
{
    const auto& [option, context] = scenario();
    const kiyosi::AnalyticEuropeanEngine engine;
    for (auto _ : state) {
        auto result = engine.price(option, context);
        benchmark::DoNotOptimize(result);
    }
}

} // namespace

BENCHMARK(BM_AnalyticEuropeanPrice);
