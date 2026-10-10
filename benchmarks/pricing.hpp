#pragma once

#include <utility>

#include <benchmark/benchmark.h>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>

namespace kiyosi::benchmark_support {

void register_monte_carlo_cases();
void register_analytics_cases();
#if KIYOSI_HAS_QUANTLIB
void register_quantlib_matrix();
#endif

template <typename Evaluate>
auto* register_calculation(const char* name, Evaluate evaluate, const char* counter)
{
    return benchmark::RegisterBenchmark(
        name, [evaluate = std::move(evaluate), counter](benchmark::State& state) {
            const auto warmup = evaluate();
            if (!warmup) {
                state.SkipWithError(warmup.error().message);
                return;
            }
            if constexpr (requires { warmup->price(); })
                state.counters[counter] = warmup->price();
            else
                state.counters[counter] = *warmup;
            for (auto _ : state) {
                auto result = evaluate();
                if (!result) {
                    state.SkipWithError(result.error().message);
                    break;
                }
                benchmark::DoNotOptimize(result);
            }
        });
}

template <typename Option, typename Engine>
auto* register_price(const char* name, Option option, Engine engine, PricingContext context)
{
    return register_calculation(name, [option = std::move(option), engine = std::move(engine), context = std::move(context)] { return engine.price(option, context); }, "price");
}

} // namespace kiyosi::benchmark_support
