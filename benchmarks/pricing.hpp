#pragma once

#include <utility>

#include <benchmark/benchmark.h>
#include <kiyosi/market/context.hpp>

namespace kiyosi::benchmark_support {

void register_monte_carlo_cases();
#if KIYOSI_HAS_QUANTLIB
void register_quantlib_matrix();
#endif

template <typename Option, typename Engine>
auto* register_price(const char* name, Option option, Engine engine, PricingContext context)
{
    return benchmark::RegisterBenchmark(
        name, [option = std::move(option), engine = std::move(engine), context = std::move(context)](
                  benchmark::State& state) {
            const auto warmup = engine.price(option, context);
            if (!warmup) {
                state.SkipWithError(warmup.error().message);
                return;
            }
            state.counters["price"] = *warmup;
            for (auto _ : state) {
                auto result = engine.price(option, context);
                if (!result) {
                    state.SkipWithError(result.error().message);
                    break;
                }
                benchmark::DoNotOptimize(result);
            }
        });
}

} // namespace kiyosi::benchmark_support
