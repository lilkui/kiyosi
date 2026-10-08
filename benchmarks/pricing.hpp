#pragma once

#include <utility>

#include <benchmark/benchmark.h>
#include <kiyosi/market/context.hpp>

namespace kiyosi::benchmark_support {

template <typename Option, typename Engine>
auto* register_price(const char* name, Option option, Engine engine, PricingContext context)
{
    return benchmark::RegisterBenchmark(
        name, [option = std::move(option), engine = std::move(engine), context = std::move(context)](
                  benchmark::State& state) {
            const auto warmup = engine.price(option, context);
            if (!warmup) {
                state.SkipWithError(warmup.error().message.c_str());
                return;
            }
            state.counters["price"] = *warmup;
            for (auto _ : state) {
                auto result = engine.price(option, context);
                benchmark::DoNotOptimize(result);
            }
        });
}

} // namespace kiyosi::benchmark_support
