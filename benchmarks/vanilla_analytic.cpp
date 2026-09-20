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
        const kiyosi::Date effective_date{std::chrono::year{2025} / 1 / 1};
        const kiyosi::Date expiry_date{std::chrono::year{2026} / 1 / 1};
        return Scenario{
            *kiyosi::make_european_option(kiyosi::OptionType::call, 100.0, effective_date, expiry_date),
            *kiyosi::make_pricing_context(
                *kiyosi::make_bsm_parameters(0.04, 0.01, 0.2), 100.0, effective_date)};
    }();
    return value;
}

void bm_analytic_european_price(benchmark::State& state)
{
    const auto& [option, context] = scenario();
    const kiyosi::AnalyticVanillaEngine engine;
    for (auto _ : state) {
        auto result = engine.price(option, context);
        benchmark::DoNotOptimize(result);
    }
}

} // namespace

BENCHMARK(bm_analytic_european_price);
