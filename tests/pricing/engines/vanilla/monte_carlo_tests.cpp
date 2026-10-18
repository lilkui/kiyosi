#include <catch2/catch_test_macros.hpp>
#include <catch2/matchers/catch_matchers_floating_point.hpp>
#include <array>
#include <bit>
#include <chrono>
#include <cmath>
#include <future>
#include <limits>
#include <random>
#include <string>
#include <type_traits>
#include <tuple>
#include <vector>
#include <kiyosi/kiyosi.hpp>
#include "support/common.hpp"
#include "support/numerical.hpp"
#include "pricing/engines/monte_carlo_host.hpp"
#include "pricing/engines/monte_carlo_mean.hpp"

TEST_CASE("CPU Monte Carlo results are invariant to thread count", "[mc-parallel]")
{
    using namespace kiyosi;
    const auto start = test::day(2025, 1, 1), end = test::day(2025, 1, 13);
    const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.2), 100.25,
                                               start_of_day(start) + std::chrono::hours{12}, all_days_calendar());
    for (const int paths : {513, 1025}) {
        for (const int threads : {0, 1, 2, 4}) {
            CAPTURE(paths, threads);
            const auto check = [&](const auto& engine, const auto& option) {
                auto settings = engine.settings();
                settings.thread_count = 1;
                const auto serial_engine = make_engine(settings, std::type_identity<std::decay_t<decltype(engine)>>{});
                REQUIRE(serial_engine);
                const auto& serial = *serial_engine;
                const auto expected = serial.price_with_greeks(option, context, true, {});
                const auto actual = engine.price_with_greeks(option, context, true, {});
                REQUIRE(expected);
                REQUIRE(actual);
                CHECK(std::bit_cast<std::uint64_t>(actual->price()) == std::bit_cast<std::uint64_t>(expected->price()));
                CHECK(actual->values_view() == expected->values_view());
                for (std::size_t index = 0; index < greek_count; ++index) {
                    const auto value = actual->values_view()[index];
                    const auto reference = expected->values_view()[index];
                    REQUIRE(value.has_value() == reference.has_value());
                    if (value)
                        CHECK(std::bit_cast<std::uint64_t>(*value) == std::bit_cast<std::uint64_t>(*reference));
                }
                const auto price = engine.price(option, context);
                REQUIRE(price);
                CHECK(*price == expected->price());
                const auto expected_vol = implied_volatility(serial, option, context, *price, {.lower_bound = 0.15, .upper_bound = 0.25});
                const auto actual_vol = implied_volatility(engine, option, context, *price, {.lower_bound = 0.15, .upper_bound = 0.25});
                REQUIRE(expected_vol);
                REQUIRE(actual_vol);
                CHECK(std::bit_cast<std::uint64_t>(*actual_vol) == std::bit_cast<std::uint64_t>(*expected_vol));
                auto pending = std::async(std::launch::async, [&] { return engine.price_with_greeks(option, context, true, {}); });
                const auto concurrent = engine.price_with_greeks(option, context, true, {});
                const auto background = pending.get();
                REQUIRE(concurrent);
                REQUIRE(background);
                CHECK(concurrent->price() == expected->price());
                CHECK(background->price() == expected->price());
                CHECK(concurrent->values_view() == expected->values_view());
                CHECK(background->values_view() == expected->values_view());
            };
            check(make_monte_carlo_vanilla_engine({paths, 7, 42, MonteCarloBackend::cpu, threads}).value(), *make_european_option(OptionType::call, 100.0, start, end));
            check(make_monte_carlo_vanilla_engine({paths, 7, 42, MonteCarloBackend::cpu, threads}).value(), *make_american_option(OptionType::put, 100.0, start, end));
            check(make_monte_carlo_vanilla_engine({paths, 7, 42, MonteCarloBackend::cpu, threads}).value(), *make_american_option(OptionType::call, 100.0, start, end));
            check(make_monte_carlo_accumulator_engine({paths, 42, MonteCarloBackend::cpu, threads}).value(),
                  *make_accumulator({.strike = 100.0, .knock_out_level = 115.0, .daily_quantity = 1.0, .acceleration_factor = 2.0, .effective_date = start, .expiry_date = end}));
            const std::vector<Date> observations{test::day(2025, 1, 3), test::day(2025, 1, 8), end};
            const TradingDayMonteCarloSettings settings{paths, 42, MonteCarloBackend::cpu, threads};
            check(make_monte_carlo_autocallable_engine(settings).value(),
                  *make_binary_snowball_option({.maturity_coupon_rate = 0.06, .barrier_state = AutocallableBarrierState::none, .effective_date = start, .expiry_date = end, .observations = *kiyosi::make_autocallable_observations(observations, std::vector<double>{110.0, 110.0, 110.0}, std::vector<double>{0.08, 0.08, 0.08})}));
            check(make_monte_carlo_autocallable_engine(settings).value(),
                  *make_ternary_snowball_option({.maturity_coupon_rate = 0.06, .minimum_coupon_rate = 0.02, .knock_in_level = 80.0, .barrier_state = AutocallableBarrierState::none, .effective_date = start, .expiry_date = end, .observations = *kiyosi::make_autocallable_observations(observations, std::vector<double>{110.0, 110.0, 110.0}, std::vector<double>{0.08, 0.08, 0.08})}));
            check(make_monte_carlo_autocallable_engine(settings).value(),
                  *make_snowball_option({.maturity_coupon_rate = 0.06, .initial_spot = 100.0, .knock_in_level = 80.0, .upper_strike = 100.0, .lower_strike = 60.0, .barrier_state = AutocallableBarrierState::none, .effective_date = start, .expiry_date = end, .observations = *kiyosi::make_autocallable_observations(observations, std::vector<double>{110.0, 110.0, 110.0}, std::vector<double>{0.08, 0.08, 0.08})}));
            check(make_monte_carlo_autocallable_engine(settings).value(),
                  *make_phoenix_option({.coupon_rate = 0.08, .initial_spot = 100.0, .knock_in_level = 80.0, .upper_strike = 100.0, .lower_strike = 60.0, .barrier_state = AutocallableBarrierState::none, .effective_date = start, .expiry_date = end, .observations = *kiyosi::make_autocallable_observations(observations, std::vector<double>{110.0, 110.0, 110.0}, std::nullopt, std::vector<double>{90.0, 90.0, 90.0})}));
        }
    }
}

TEST_CASE("Parallel American Monte Carlo agrees with finite differences", "[mc-parallel]")
{
    using namespace kiyosi;
    const auto start = test::day(2025, 1, 1), end = test::day(2026, 1, 1);
    const auto context = *make_pricing_context(*make_bsm_parameters(0.05, 0.02, 0.2), 100.0, start);
    for (const auto type : {OptionType::put, OptionType::call}) {
        const auto option = *make_american_option(type, 100.0, start, end);
        const auto reference = make_finite_difference_vanilla_engine({1200, 1200}).value().price(option, context);
        const auto serial = make_monte_carlo_vanilla_engine({100'000, 50, 42, MonteCarloBackend::cpu, 1}).value().price(option, context);
        REQUIRE(reference);
        REQUIRE(serial);
        CHECK_THAT(*serial, Catch::Matchers::WithinAbs(*reference, 0.15));
        for (const int threads : {0, 2, 4}) {
            CAPTURE(type, threads);
            const auto parallel = make_monte_carlo_vanilla_engine({100'000, 50, 42, MonteCarloBackend::cpu, threads}).value().price(option, context);
            REQUIRE(parallel);
            CHECK(std::bit_cast<std::uint64_t>(*parallel) == std::bit_cast<std::uint64_t>(*serial));
        }
    }
}

TEST_CASE("Monte Carlo factories reject invalid thread counts", "[mc-parallel][engine-creation]")
{
    using namespace kiyosi;
    CHECK(MonteCarloSettings{}.thread_count == 0);
    CHECK(TradingDayMonteCarloSettings{}.thread_count == 0);
    const auto check = [](const auto& result) {
        REQUIRE_FALSE(result);
        CHECK(result.error().category == ErrorCategory::invalid_parameter);
    };
    for (const auto backend : {MonteCarloBackend::cpu, MonteCarloBackend::cuda}) {
        for (const int threads : {-1, 1, 4}) {
            if (backend == MonteCarloBackend::cpu && threads > 0)
                continue;
            CAPTURE(backend, threads);
            check(make_monte_carlo_vanilla_engine({513, 7, 42, backend, threads}));
            check(make_monte_carlo_accumulator_engine({513, 42, backend, threads}));
            check(make_monte_carlo_autocallable_engine({513, 42, backend, threads}));
        }
    }
}

#if KIYOSI_HAS_CUDA
#include <cuda_runtime.h>

TEST_CASE("Unavailable European CUDA Greeks retain the scalar memory footprint", "[cuda][mc-shared-samples]")
{
    using namespace kiyosi;
    const auto start = test::day(2025, 1, 1), end = test::day(2026, 1, 1);
    const auto option = *make_european_option(OptionType::put, 100.0, start, end);
    const auto context = *make_pricing_context(*make_bsm_parameters(0.05, 0.02, 0.2), 100.0, start);
    const auto engine = make_monte_carlo_vanilla_engine({100'001, 50, 42, MonteCarloBackend::cuda}).value();
    REQUIRE(engine.price(option, context));
    int device = 0;
    cudaMemPool_t pool{};
    REQUIRE(cudaGetDevice(&device) == cudaSuccess);
    REQUIRE(cudaDeviceGetDefaultMemPool(&pool, device) == cudaSuccess);
    const auto measure = [&](const auto& evaluate) {
        std::uint64_t peak = 0;
        REQUIRE(cudaMemPoolSetAttribute(pool, cudaMemPoolAttrUsedMemHigh, &peak) == cudaSuccess);
        const auto result = evaluate();
        REQUIRE(result);
        REQUIRE(cudaMemPoolGetAttribute(pool, cudaMemPoolAttrUsedMemHigh, &peak) == cudaSuccess);
        return std::pair{*result, peak};
    };
    const auto scalar = measure([&] { return engine.price(option, context); });
    const auto unavailable = measure([&] {
        return engine.price_with_greeks(option, context, {Greek::delta}, {.spot_shift = 200.0});
    });
    CHECK(unavailable.first.price() == scalar.first);
    CHECK_FALSE(unavailable.first.has(Greek::delta));
    CHECK(unavailable.second == scalar.second);
}

TEST_CASE("CUDA Greek workspaces preserve scalar stencils and concurrent requests", "[cuda][cuda-workspace]")
{
    using namespace kiyosi;
    const auto start = test::day(2025, 1, 1), end = test::day(2025, 1, 13);
    const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.2), 100.25,
                                               start_of_day(start) + std::chrono::hours{12}, all_days_calendar());
    const auto check = [&](const auto& engine, const auto& option) {
        const auto scalar = detail::price_with_greeks(engine, option, context, true, {});
        REQUIRE(scalar);
        for (const GreeksRequest request : {GreeksRequest{true}, GreeksRequest{Greek::delta, Greek::gamma}, GreeksRequest{Greek::vanna}, GreeksRequest{Greek::zomma}}) {
            const auto result = engine.price_with_greeks(option, context, request, {});
            REQUIRE(result);
            CHECK(result->price() == scalar->price());
            CHECK(result->values_view() == scalar->selected(request).values_view());
        }
        auto first = std::async(std::launch::async, [&] { return engine.price_with_greeks(option, context, true, {}); });
        auto second = std::async(std::launch::async, [&] { return engine.price_with_greeks(option, context, true, {}); });
        for (auto* future : {&first, &second}) {
            const auto result = future->get();
            REQUIRE(result);
            CHECK(result->price() == scalar->price());
            CHECK(result->values_view() == scalar->values_view());
        }
    };
    const auto vanilla = make_monte_carlo_vanilla_engine({31, 5, 42, MonteCarloBackend::cuda}).value();
    check(vanilla, *make_european_option(OptionType::call, 100.0, start, end));
    check(vanilla, *make_american_option(OptionType::put, 100.0, start, end));
    check(make_monte_carlo_accumulator_engine({31, 42, MonteCarloBackend::cuda}).value(),
          *make_accumulator({.strike = 100.0, .knock_out_level = 115.0, .daily_quantity = 1.0, .acceleration_factor = 2.0, .effective_date = start, .expiry_date = end}));
    const std::vector<Date> observations{test::day(2025, 1, 3), test::day(2025, 1, 8), end};
    const TradingDayMonteCarloSettings settings{31, 42, MonteCarloBackend::cuda};
    check(make_monte_carlo_autocallable_engine(settings).value(),
          *make_binary_snowball_option({.maturity_coupon_rate = 0.06, .barrier_state = AutocallableBarrierState::none, .effective_date = start, .expiry_date = end, .observations = *kiyosi::make_autocallable_observations(observations, std::vector<double>{110.0, 110.0, 110.0}, std::vector<double>{0.08, 0.08, 0.08})}));
    check(make_monte_carlo_autocallable_engine(settings).value(),
          *make_ternary_snowball_option({.maturity_coupon_rate = 0.06, .minimum_coupon_rate = 0.02, .knock_in_level = 80.0, .barrier_state = AutocallableBarrierState::none, .effective_date = start, .expiry_date = end, .observations = *kiyosi::make_autocallable_observations(observations, std::vector<double>{110.0, 110.0, 110.0}, std::vector<double>{0.08, 0.08, 0.08})}));
    check(make_monte_carlo_autocallable_engine(settings).value(),
          *make_snowball_option({.maturity_coupon_rate = 0.06, .initial_spot = 100.0, .knock_in_level = 80.0, .upper_strike = 100.0, .lower_strike = 60.0, .barrier_state = AutocallableBarrierState::none, .effective_date = start, .expiry_date = end, .observations = *kiyosi::make_autocallable_observations(observations, std::vector<double>{110.0, 110.0, 110.0}, std::vector<double>{0.08, 0.08, 0.08})}));
    check(make_monte_carlo_autocallable_engine(settings).value(),
          *make_phoenix_option({.coupon_rate = 0.08, .initial_spot = 100.0, .knock_in_level = 80.0, .upper_strike = 100.0, .lower_strike = 60.0, .barrier_state = AutocallableBarrierState::none, .effective_date = start, .expiry_date = end, .observations = *kiyosi::make_autocallable_observations(observations, std::vector<double>{110.0, 110.0, 110.0}, std::nullopt, std::vector<double>{90.0, 90.0, 90.0})}));
}

TEST_CASE("CUDA packed path outputs preserve block boundaries and failure flags", "[cuda][cuda-workspace]")
{
    using namespace kiyosi::detail;
    for (const int count : {2, 256, 258, 65'538, 100'002}) {
        CAPTURE(count);
        CudaEuropeanRequest request{count, 42, 100.0, 90.0, 0.0, 0.0, 1};
        const auto valid = cuda_european_price(request);
        REQUIRE(valid.status == CudaPricingStatus::success);
        CHECK(valid.payoff_mean == 10.0);
        request.drift = 1000.0;
        CHECK(cuda_european_price(request).status == CudaPricingStatus::invalid_result);
        request.drift = 0.0;
        const auto recovered = cuda_european_price(request);
        REQUIRE(recovered.status == CudaPricingStatus::success);
        CHECK(recovered.payoff_mean == 10.0);
    }
    for (const int count : {1, 255, 257, 65'537}) {
        const auto result = cuda_autocallable_price(
            {.path_count = count, .seed = 42, .spot = 100.0, .terminal_discount = {0.9}, .program = {.principal_ratio = 2.0, .terminal_kind = AutocallableTerminalKind::fixed}}, {});
        REQUIRE(result.status == CudaPricingStatus::success);
        CHECK(result.payoff_mean == 1.8);
    }
}
#endif

#if KIYOSI_HAS_CUDA
#include <cuda_runtime_api.h>
#endif

TEST_CASE("Trading-day random generator matches the PCG32 reference sequence", "[monte-carlo-performance]")
{
    kiyosi::detail::Pcg32 generator{42, 54};
    for (const auto expected : std::array<std::uint32_t, 6>{0xa15c02b7, 0x7b47f409, 0xba1d3330,
                                                            0x83d2f293, 0xbfa4784b, 0xcbed606e})
        CHECK(generator() == expected);
}

TEST_CASE("Trading-day path streams preserve normal moments and independent prefixes", "[monte-carlo-performance]")
{
    constexpr int count = 100'000;
    for (const auto seed : {std::uint64_t{42}, std::numeric_limits<std::uint64_t>::max()}) {
        CAPTURE(seed);
        std::vector<double> prefixes;
        prefixes.reserve(count);
        double squares = 0.0, products = 0.0;
        const auto first = kiyosi::detail::cpu_path_mean({count, seed, kiyosi::MonteCarloBackend::cpu, 1}, [&](auto& generator) -> kiyosi::Result<double> {
            std::normal_distribution<double> normal; // NOLINT(misc-const-correctness): sampling mutates the distribution cache.
            const double value = normal(generator);
            const double next = normal(generator);
            prefixes.push_back(value);
            squares += value * value;
            products += value * next;
            return value;
        });
        REQUIRE(first);
        CHECK_THAT(*first, Catch::Matchers::WithinAbs(0.0, 0.02));
        CHECK_THAT(squares / count, Catch::Matchers::WithinAbs(1.0, 0.02));
        CHECK_THAT(products / count, Catch::Matchers::WithinAbs(0.0, 0.02));
        std::size_t path = 0;
        const auto shortened = kiyosi::detail::cpu_path_mean({count, seed, kiyosi::MonteCarloBackend::cpu, 1}, [&](auto& generator) -> kiyosi::Result<double> {
            std::normal_distribution<double> normal; // NOLINT(misc-const-correctness): sampling mutates the distribution cache.
            const double value = normal(generator);
            REQUIRE(value == prefixes[path++]);
            return value;
        });
        REQUIRE(shortened);
        CHECK(*shortened == *first);
    }
}

TEST_CASE("CPU path batches preserve scalar streams and propagate payoff failures", "[monte-carlo-performance]")
{
    using namespace kiyosi;
    const TradingDayMonteCarloSettings settings{16, 42, MonteCarloBackend::cpu, 1};
    const auto scalar = detail::cpu_path_mean(settings, [](auto& generator) -> Result<double> {
        return static_cast<double>(generator());
    });
    const auto batch = detail::cpu_path_means<2>(settings, [](auto& generator) -> Result<std::array<double, 2>> {
        const auto first = generator();
        for (std::uint32_t draw = 0; draw < first % 7; ++draw)
            static_cast<void>(generator());
        return std::array{static_cast<double>(first), 10.0};
    });
    REQUIRE(scalar);
    REQUIRE(batch);
    CHECK((*batch)[0] == *scalar);
    CHECK((*batch)[1] == 10.0);

    int calls = 0;
    const auto failed = detail::cpu_path_means<2>(settings, [&](auto&) -> Result<std::array<double, 2>> {
        if (++calls == 3)
            return std::unexpected(Error{ErrorCategory::invalid_schedule, "payoff failed"});
        return std::array{1.0, 2.0};
    });
    REQUIRE_FALSE(failed);
    CHECK(failed.error().category == ErrorCategory::invalid_schedule);
    CHECK(failed.error().message == "payoff failed");
    CHECK(calls == 3);
}

TEST_CASE("Monte Carlo sample accumulation matches partial merging across scales", "[mc-mean]")
{
    using kiyosi::detail::MonteCarloMean;
    MonteCarloMean direct{}, merged{};
    const auto check = [&](double sample) {
        direct.add(sample);
        int exponent = 0;
        const double fraction = std::frexp(sample, &exponent);
        merged.merge({fraction, 0.0, exponent, 1});
        CHECK(direct.sum == merged.sum);
        CHECK(direct.correction == merged.correction);
        CHECK(direct.exponent == merged.exponent);
        CHECK(direct.count == merged.count);
        CHECK(direct.value() == merged.value());
    };
    const double maximum = std::numeric_limits<double>::max();
    const double minimum = std::numeric_limits<double>::denorm_min();
    for (const double sample : {0.0, maximum, maximum, -maximum, -maximum, minimum, minimum, -minimum, -0.0, 10.0, -10.0})
        check(sample);
    std::mt19937_64 generator{42}; // NOLINT(bugprone-random-generator-seed): regression samples must be reproducible.
    std::uniform_real_distribution<double> fraction{-1.0, 1.0};
    std::uniform_int_distribution<int> exponent{-1073, 1024};
    for (int sample = 0; sample < 2048; ++sample)
        check(std::ldexp(fraction(generator), exponent(generator)));
}

TEST_CASE("Monte Carlo averages retain finite extreme and subnormal payoffs", "[audit-fixes][mc-mean]")
{
    using kiyosi::detail::MonteCarloMean;
    const double maximum = std::numeric_limits<double>::max();
    for (const double payoff : {maximum, -maximum, std::numeric_limits<double>::denorm_min()}) {
        MonteCarloMean mean{};
        for (int path = 0; path < 100000; ++path)
            mean.add(payoff);
        CHECK(mean.value() == payoff);
    }
    MonteCarloMean positive{}, negative{};
    for (int path = 0; path < 2; ++path) {
        positive.add(maximum);
        negative.add(-maximum);
    }
    positive.merge(negative);
    CHECK(positive.value() == 0.0);
    MonteCarloMean unequal{}, repeated{};
    unequal.add(10.0);
    for (int path = 0; path < 3; ++path)
        repeated.add(20.0);
    unequal.merge(repeated);
    CHECK(unequal.value() == 17.5);
    MonteCarloMean tiny{};
    tiny.add(std::numeric_limits<double>::denorm_min());
    for (int path = 0; path < 999; ++path)
        tiny.add(0.0);
    CHECK(tiny.value() == 0.0);
    positive.add(std::numeric_limits<double>::denorm_min() * 10.0);
    CHECK(positive.value() == std::numeric_limits<double>::denorm_min() * 2.0);
}

TEST_CASE("CUDA host adapter preserves results and failure categories")
{
    using namespace kiyosi;
    using namespace kiyosi::detail;
    REQUIRE(cuda_mean({CudaPricingStatus::success, 42.0, nullptr}).value() == 42.0);
    for (const auto [status, category] : {
             std::pair{CudaPricingStatus::unavailable, ErrorCategory::backend_unavailable},
             std::pair{CudaPricingStatus::failure, ErrorCategory::backend_failure},
             std::pair{CudaPricingStatus::invalid_result, ErrorCategory::invalid_result}}) {
        const auto result = cuda_mean({status, 0.0, "backend diagnostic"});
        REQUIRE_FALSE(result.has_value());
        REQUIRE(result.error() == Error{category, "backend diagnostic"});
    }
    REQUIRE_THROWS_AS(static_cast<void>(cuda_mean({CudaPricingStatus::out_of_memory, 0.0, nullptr})), std::bad_alloc);
    const auto unknown = cuda_mean({static_cast<CudaPricingStatus>(255), 0.0, nullptr});
    REQUIRE_FALSE(unknown.has_value());
    REQUIRE(unknown.error().category == ErrorCategory::backend_failure);
}

namespace {

using kiyosi::test::day;

void check_american_currency_scale(kiyosi::MonteCarloBackend backend)
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1);
    const auto end = day(2026, 1, 1);
    const auto parameters = *make_bsm_parameters(0.1, 0.0, 0.3);
    const auto engine = make_monte_carlo_vanilla_engine({20'000, 50, 47, backend}).value();
    const auto base = engine.price(*make_american_option(OptionType::put, 100.0, start, end),
                                   *make_pricing_context(parameters, 90.0, start));
    REQUIRE(base);
    for (const double scale : {1e-300, 1e-15, 1e12, 1e15, 1e303}) {
        CAPTURE(backend, scale);
        const auto price = engine.price(*make_american_option(OptionType::put, 100.0 * scale, start, end),
                                        *make_pricing_context(parameters, 90.0 * scale, start));
        REQUIRE(price);
        CHECK_THAT(*price / scale, Catch::Matchers::WithinAbs(*base, 1e-7));
    }
}

void check_finite_payoff_averages(kiyosi::MonteCarloBackend backend)
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1);
    const auto end = day(2026, 1, 1);
    const auto parameters = *make_bsm_parameters(0.0, 0.0, 0.2);
    const auto vanilla = make_monte_carlo_vanilla_engine({100'000, 2, 42, backend}).value();
    for (const auto type : {OptionType::call, OptionType::put}) {
        const auto price = [&](double scale) {
            return vanilla.price(*make_european_option(type, 100.0 * scale, start, end),
                                 *make_pricing_context(parameters, 100.0 * scale, start));
        };
        const auto base = price(1.0);
        REQUIRE(base);
        for (const double scale : {1e-300, 1e303}) {
            CAPTURE(backend, type, scale);
            const auto scaled = price(scale);
            REQUIRE(scaled);
            CHECK_THAT(*scaled / scale, Catch::Matchers::WithinRel(*base, 1e-12));
        }
    }
    const auto next_day = start + std::chrono::days{1};
    const auto accumulator = make_monte_carlo_accumulator_engine({100'000, 42, backend}).value();
    for (const double strike : {90.0, 110.0}) {
        const auto price = [&](double scale) {
            const auto option = *make_accumulator(
                {.strike = strike * scale, .knock_out_level = 200.0 * scale, .daily_quantity = 0.0, .acceleration_factor = 1.0, .accumulated_quantity = 1.0, .effective_date = start, .expiry_date = next_day});
            return accumulator.price(option, *make_pricing_context(parameters, 100.0 * scale, start));
        };
        const auto base = price(1.0);
        const auto scaled = price(1e303);
        REQUIRE(base);
        REQUIRE(scaled);
        CHECK_THAT(*scaled / 1e303, Catch::Matchers::WithinRel(*base, 1e-12));
    }
    const auto autocallable = make_monte_carlo_autocallable_engine({100'000, 42, backend}).value();
    for (const double principal : {1e305, std::numeric_limits<double>::denorm_min()}) {
        const auto option = *make_binary_snowball_option(
            {.maturity_coupon_rate = 0.0, .principal_ratio = principal, .effective_date = start, .expiry_date = next_day, .observations = {{next_day, 120.0, 0.0}}});
        const auto price = autocallable.price(option, *make_pricing_context(parameters, 100.0, start));
        REQUIRE(price);
        CHECK(*price == principal);
    }
    const auto overflow = *make_binary_snowball_option(
        {.maturity_coupon_rate = 1e308, .principal_ratio = 1e308, .effective_date = start, .expiry_date = end, .observations = {{end, 120.0, 1e308}}});
    const auto invalid = make_monte_carlo_autocallable_engine({32, 42, backend}).value().price(overflow, *make_pricing_context(parameters, 100.0, start));
    REQUIRE_FALSE(invalid);
    CHECK(invalid.error().category == ErrorCategory::invalid_result);
}

TEST_CASE("CPU Monte Carlo averages finite payoffs without overflowing", "[audit-fixes]")
{
    check_finite_payoff_averages(kiyosi::MonteCarloBackend::cpu);
}

#if KIYOSI_HAS_CUDA
TEST_CASE("CUDA Monte Carlo averages finite payoffs without overflowing", "[cuda][audit-fixes]")
{
    check_finite_payoff_averages(kiyosi::MonteCarloBackend::cuda);
}
#endif

TEST_CASE("American Monte Carlo prices are invariant to currency scale", "[mc-regression]")
{
    check_american_currency_scale(kiyosi::MonteCarloBackend::cpu);
}

TEST_CASE("Monte Carlo engines are deterministic, validated, and price vanilla options")
{
    const auto valuation = day(2025, 1, 1);
    const auto expiry_date = valuation + std::chrono::days{365};
    const auto parameters = *kiyosi::make_bsm_parameters(0.05, 0.02, 0.2);
    const auto context = *kiyosi::make_pricing_context(parameters, 100.0, valuation);
    const auto call = *kiyosi::make_european_option(kiyosi::OptionType::call, 100.0, valuation, expiry_date);
    const auto american = *kiyosi::make_american_option(kiyosi::OptionType::put, 100.0, valuation, expiry_date);

    const auto european = kiyosi::make_monte_carlo_vanilla_engine({20'000, 2, 42}).value();
    const auto first = european.price(call, context);
    const auto second = european.price(call, context);
    REQUIRE(first.has_value());
    REQUIRE(second.has_value());
    CHECK(*first == *second);
    CHECK(std::abs(*first -
                   *kiyosi::AnalyticVanillaEngine{}.price(call, context)) < 0.5);

    const auto american_engine = kiyosi::make_monte_carlo_vanilla_engine({20'000, 20, 42}).value();
    const auto american_result = american_engine.price(american, context);
    REQUIRE(american_result.has_value());
    CHECK(*american_result >= 0.0);

    CHECK_FALSE(kiyosi::make_monte_carlo_vanilla_engine({0, 2}).has_value());
    CHECK_FALSE(kiyosi::make_monte_carlo_vanilla_engine({20, 2}).value().price(american, context).has_value());
}

TEST_CASE("Vanilla Monte Carlo validates generic settings at creation", "[audit-fixes]")
{
    using namespace kiyosi;
    const auto effective = day(2025, 1, 1);
    const auto expiry = day(2026, 1, 1);
    const auto parameters = *make_bsm_parameters(0.05, 0.02, 0.2);
    const auto check = [&](const auto& option) {
        for (const MonteCarloSettings settings : {
                 MonteCarloSettings{0, 2}, {-1, 2}, {10'000'001, 2}, {1, 0}, {1, 1}, {1, 10'001}, {1, 2, 1, static_cast<MonteCarloBackend>(255)}}) {
            const auto result = make_monte_carlo_vanilla_engine(settings);
            REQUIRE_FALSE(result);
            CHECK(result.error().category == ErrorCategory::invalid_parameter);
        }
        for (const auto backend : {MonteCarloBackend::cpu, MonteCarloBackend::cuda})
            for (const auto settings : {MonteCarloSettings{1, 2, 1, backend}, {10'000'000, 10'000, 1, backend}}) {
                const auto result = make_monte_carlo_vanilla_engine(settings).value().price(
                    option, *make_pricing_context(parameters, 110.0, expiry));
                REQUIRE(result);
                CHECK(*result == 10.0);
            }
    };
    check(*make_european_option(OptionType::call, 100.0, effective, expiry));
    check(*make_american_option(OptionType::call, 100.0, effective, expiry));
}

TEST_CASE("Monte Carlo engines return intrinsic value at expiry_date")
{
    const auto expiry_date = day(2025, 1, 1);
    const auto parameters = *kiyosi::make_bsm_parameters(0.05, 0.02, 0.2);
    const auto context = *kiyosi::make_pricing_context(parameters, 110.0, expiry_date);
    const auto call = *kiyosi::make_european_option(kiyosi::OptionType::call, 100.0, expiry_date, expiry_date);
    const auto result = kiyosi::make_monte_carlo_vanilla_engine({10, 2, 1}).value().price(call, context);
    REQUIRE(result.has_value());
    CHECK(*result == 10.0);
}

TEST_CASE("Monte Carlo engines preserve timestamp and supported date boundaries")
{
    const auto first = kiyosi::Date{std::chrono::year::min() / std::chrono::January / 1};
    const auto last = kiyosi::Date{std::chrono::year::max() / std::chrono::December / 31};
    const auto parameters = *kiyosi::make_bsm_parameters(0.0, 0.0, 0.2);
    const auto engine = kiyosi::make_monte_carlo_vanilla_engine({32, 3, 7}).value();
    for (const auto effective : {first, day(2025, 1, 1), last - std::chrono::days{1}}) {
        const auto expiry = effective + std::chrono::days{1};
        const auto european = *kiyosi::make_european_option(
            kiyosi::OptionType::call, 100.0, effective, expiry);
        const auto american = *kiyosi::make_american_option(
            kiyosi::OptionType::call, 100.0, effective, expiry);
        const auto check = [&](const auto& option) {
            for (const auto time : {kiyosi::start_of_day(effective),
                                    kiyosi::start_of_day(effective) + std::chrono::hours{12},
                                    kiyosi::start_of_day(expiry)}) {
                const auto context = *kiyosi::make_pricing_context(parameters, 110.0, time);
                const auto result = engine.price(option, context);
                REQUIRE(result);
                CHECK(std::isfinite(*result));
                if (time == expiry) CHECK(*result == 10.0);
            }
            for (const auto time : {kiyosi::start_of_day(effective) - std::chrono::microseconds{1},
                                    kiyosi::start_of_day(expiry) + std::chrono::hours{12}}) {
                const auto context = kiyosi::make_pricing_context(parameters, 110.0, time);
                if (!context) {
                    CHECK(context.error().category == kiyosi::ErrorCategory::invalid_date);
                    continue;
                }
                const auto result = engine.price(option, *context);
                REQUIRE_FALSE(result);
                CHECK(result.error().category == kiyosi::ErrorCategory::invalid_time_range);
            }
        };
        check(european);
        check(american);
    }
}

TEST_CASE("European Monte Carlo samples terminal prices independently of the time grid", "[audit-fixes]")
{
    const auto valuation = day(2025, 1, 1);
    const auto expiry_date = valuation + std::chrono::days{365};
    const auto parameters = *kiyosi::make_bsm_parameters(0.05, 0.02, 0.2);
    const auto context = *kiyosi::make_pricing_context(parameters, 100.0, valuation);
    for (const auto type : {kiyosi::OptionType::call, kiyosi::OptionType::put}) {
        const auto option = *kiyosi::make_european_option(type, 100.0, valuation, expiry_date);
        const auto baseline = kiyosi::make_monte_carlo_vanilla_engine({20'000, 2, 42}).value().price(option, context);
        REQUIRE(baseline);
        CHECK_THAT(*baseline, Catch::Matchers::WithinAbs(*kiyosi::AnalyticVanillaEngine{}.price(option, context), 0.5));
        const auto different_seed = kiyosi::make_monte_carlo_vanilla_engine({20'000, 2, 43}).value().price(option, context);
        REQUIRE(different_seed);
        CHECK(*different_seed != *baseline);
        for (const int step_count : {2, 7, 50, 10'000}) {
            CAPTURE(type, step_count);
            const auto engine = kiyosi::make_monte_carlo_vanilla_engine({19'999, step_count, 42}).value();
            const auto price = engine.price(option, context);
            REQUIRE(price);
            CHECK(*price == *baseline);
            CHECK(engine.settings().step_count == step_count);
        }
        for (const auto backend : {kiyosi::MonteCarloBackend::cpu, kiyosi::MonteCarloBackend::cuda}) {
            for (const int step_count : {1, 10'001}) {
                const auto invalid = kiyosi::make_monte_carlo_vanilla_engine({10, step_count, 42, backend});
                REQUIRE_FALSE(invalid);
                CHECK(invalid.error().category == kiyosi::ErrorCategory::invalid_parameter);
            }
        }
    }
}

TEST_CASE("European Monte Carlo implied volatility reuses samples without changing results", "[mc-implied-samples]")
{
    using namespace kiyosi;
    struct ScalarEngine {
        MonteCarloVanillaEngine engine;
        Result<double> price(const EuropeanOption& option, const PricingContext& context) const
        {
            return engine.price(option, context);
        }
    };
    const auto start = day(2025, 1, 1), end = day(2026, 1, 1);
    for (const auto type : {OptionType::call, OptionType::put}) {
        const auto option = *make_european_option(type, 100.0, start, end);
        for (const int paths : {1, 127, 128, 1001})
            for (const auto seed : {std::uint64_t{42}, std::numeric_limits<std::uint64_t>::max()})
                for (const auto valuation : {start_of_day(start), start_of_day(start) + std::chrono::hours{12}, start_of_day(end)}) {
                    CAPTURE(type, paths, seed, valuation.time_since_epoch().count());
                    const auto engine = make_monte_carlo_vanilla_engine({paths, 50, seed}).value();
                    const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.2), 100.25, valuation);
                    const auto quote = engine.price(option, context);
                    REQUIRE(quote);
                    for (const double observed : {*quote, -1.0})
                        for (const auto settings : {ImpliedVolatilitySettings{},
                                                    ImpliedVolatilitySettings{.lower_bound = 0.1, .upper_bound = 0.4},
                                                    ImpliedVolatilitySettings{.max_iterations = 1},
                                                    ImpliedVolatilitySettings{.upper_bound = 1000.0}}) {
                            const auto expected = implied_volatility(ScalarEngine{engine}, option, context, observed, settings);
                            const auto actual = implied_volatility(engine, option, context, observed, settings);
                            REQUIRE(actual.has_value() == expected.has_value());
                            if (expected) {
                                CHECK(*actual == *expected);
                            } else
                                CHECK(actual.error().category == expected.error().category);
                        }
                }
    }
    const auto option = *make_european_option(OptionType::call, 100.0, start, end);
    const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.2), 100.25, start);
    const auto engine = make_monte_carlo_vanilla_engine({1001, 50, 42}).value();
    const double quote = *engine.price(option, context);
    // Sparse samples need a local bracket; high-volatility tails can miss the quote.
    const ImpliedVolatilitySettings bounds{.lower_bound = 0.1, .upper_bound = 0.4};
    const auto expected = implied_volatility(engine, option, context, quote, bounds);
    REQUIRE(expected);
    CHECK_THAT(*expected, Catch::Matchers::WithinAbs(0.2, 1e-8));
    std::array<std::future<Result<double>>, 2> concurrent;
    for (auto& task : concurrent)
        task = std::async(std::launch::async, [&] { return implied_volatility(engine, option, context, quote, bounds); });
    for (auto& task : concurrent) {
        const auto actual = task.get();
        REQUIRE(actual);
        CHECK(*actual == *expected);
    }
    const auto unseeded = make_monte_carlo_vanilla_engine({1001, 50}).value();
    const auto random = implied_volatility(unseeded, option, context, quote);
    // A fresh sampled curve need not bracket a quote from another seed.
    if (random)
        CHECK(std::isfinite(*random));
    else
        CHECK(random.error().category == ErrorCategory::unbracketed_volatility);
    CHECK_FALSE(unseeded.settings().seed);
    const auto invalid_paths = make_monte_carlo_vanilla_engine({0, 50, 42});
    REQUIRE_FALSE(invalid_paths);
    CHECK(invalid_paths.error().category == ErrorCategory::invalid_parameter);
    const auto invalid_settings = implied_volatility(engine, option, context, quote, {.lower_bound = 0.0});
    REQUIRE_FALSE(invalid_settings);
    CHECK(invalid_settings.error().category == ErrorCategory::invalid_parameter);
    const auto invalid_quote = implied_volatility(engine, option, context, std::numeric_limits<double>::infinity());
    REQUIRE_FALSE(invalid_quote);
    CHECK(invalid_quote.error().category == ErrorCategory::invalid_parameter);
}

TEST_CASE("American implied volatility retains scalar trials and request isolation", "[mc-american-implied]")
{
    using namespace kiyosi;
    struct ScalarEngine {
        MonteCarloVanillaEngine engine;
        Result<double> price(const AmericanOption& option, const PricingContext& context) const
        {
            return engine.price(option, context);
        }
    };
    const auto start = day(2025, 1, 1), end = day(2026, 1, 1);
    const auto context = *make_pricing_context(*make_bsm_parameters(0.05, 0.02, 0.2), 100.25, start);
    for (const auto backend : {MonteCarloBackend::cpu, MonteCarloBackend::cuda}) {
#if !KIYOSI_HAS_CUDA
        if (backend == MonteCarloBackend::cuda) continue;
#endif
        for (const auto type : {OptionType::call, OptionType::put}) {
            const auto option = *make_american_option(type, 100.0, start, end);
            for (const int paths : {127, 128, 1001}) {
                const auto engine = make_monte_carlo_vanilla_engine({paths, 7, 42, backend}).value();
                const double quote = *engine.price(option, context);
                for (const double observed : {quote, -1.0})
                    for (const auto settings : {ImpliedVolatilitySettings{},
                                                ImpliedVolatilitySettings{.lower_bound = 0.1, .upper_bound = 0.4},
                                                ImpliedVolatilitySettings{.max_iterations = 1},
                                                ImpliedVolatilitySettings{.upper_bound = 1000.0}}) {
                        CAPTURE(backend, type, paths, observed);
                        const auto expected = implied_volatility(ScalarEngine{engine}, option, context, observed, settings);
                        const auto actual = implied_volatility(engine, option, context, observed, settings);
                        REQUIRE(actual.has_value() == expected.has_value());
                        if (expected) CHECK(*actual == *expected);
                        else CHECK(actual.error().category == expected.error().category);
                    }
                const auto expected = implied_volatility(engine, option, context, quote);
                REQUIRE(expected);
                auto task = std::async(std::launch::async, [&] { return implied_volatility(engine, option, context, quote); });
                const auto concurrent = implied_volatility(engine, option, context, quote);
                const auto result = task.get();
                REQUIRE(result);
                REQUIRE(concurrent);
                CHECK(*result == *expected);
                CHECK(*concurrent == *expected);
            }
        }
    }
    const auto option = *make_american_option(OptionType::put, 100.0, start, end);
    const auto unseeded = make_monte_carlo_vanilla_engine({1001, 7}).value();
    const auto random = implied_volatility(unseeded, option, context, 7.0);
    if (random) CHECK(std::isfinite(*random));
    else CHECK(random.error().category == ErrorCategory::unbracketed_volatility);
    CHECK_FALSE(unseeded.settings().seed);
}

TEST_CASE("European Monte Carlo shared samples retain scalar Greek stencils", "[mc-shared-samples]")
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1), end = day(2026, 1, 1);
    for (const auto type : {OptionType::call, OptionType::put}) {
        const auto option = *make_european_option(type, 100.0, start, end);
        for (const int paths : {1, 127, 128, 510, 511, 512, 513, 1001, 1023, 1024, 1025}) {
            for (const auto seed : {std::uint64_t{42}, std::numeric_limits<std::uint64_t>::max()}) {
                const auto engine = make_monte_carlo_vanilla_engine({paths, 50, seed}).value();
                for (const auto valuation : {start_of_day(start), start_of_day(start) + std::chrono::hours{12},
                                             start_of_day(end) - std::chrono::microseconds{1}, start_of_day(end)}) {
                    const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.2), 100.25, valuation);
                    for (const auto shifts : {NumericalShiftSettings{}, NumericalShiftSettings{0.125, 0.02, 0.005, 2}}) {
                        const auto actual = kiyosi::test::price_differences(engine, option, context, shifts);
                        const auto expected = detail::price_with_greeks(engine, option, context, true, shifts);
                        REQUIRE(actual);
                        REQUIRE(expected);
                        CHECK(actual->price() == expected->price());
                        CHECK(actual->values_view() == expected->values_view());
                        const auto selected = engine.price_with_greeks(option, context, {Greek::delta, Greek::gamma}, shifts);
                        REQUIRE(selected);
                        CHECK(selected->values_view() == actual->selected({Greek::delta, Greek::gamma}).values_view());
                    }
                }
            }
        }
    }
}

TEST_CASE("European Monte Carlo shared samples preserve failures and concurrent calls", "[mc-shared-samples]")
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1), end = day(2026, 1, 1);
    const auto option = *make_european_option(OptionType::call, 100.0, start, end);
    const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.2), 100.25, start);
    const auto engine = make_monte_carlo_vanilla_engine({1025, 50, 42, MonteCarloBackend::cpu, 4}).value();
    const auto invalid_context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 1000.0), 100.25, start);
    const auto invalid = engine.price_with_greeks(option, invalid_context, true);
    REQUIRE_FALSE(invalid);
    CHECK(invalid.error().category == ErrorCategory::invalid_result);
    NumericalShiftSettings shifts;
    shifts.rate_shift = 1000.0;
    const auto bumped = engine.price_with_greeks(option, context, {Greek::rho}, shifts);
    REQUIRE_FALSE(bumped);
    CHECK(bumped.error().category == ErrorCategory::invalid_result);
    const auto expected = engine.price_with_greeks(option, context, true);
    REQUIRE(expected);
    std::array<std::future<Result<PricingResult>>, 2> concurrent;
    for (auto& task : concurrent)
        task = std::async(std::launch::async, [&] { return engine.price_with_greeks(option, context, true); });
    for (auto& task : concurrent) {
        const auto result = task.get();
        REQUIRE(result);
        CHECK(result->price() == expected->price());
        CHECK(result->values_view() == expected->values_view());
    }
    const auto unseeded = make_monte_carlo_vanilla_engine({127, 50}).value();
    const auto result = unseeded.price_with_greeks(option, context, true);
    REQUIRE(result);
    CHECK(result->all_finite());
    CHECK_FALSE(unseeded.settings().seed);
}

TEST_CASE("European Monte Carlo unavailable stencils retain scalar prices", "[mc-shared-samples]")
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1), end = day(2026, 1, 1);
    const auto context = *make_pricing_context(*make_bsm_parameters(0.05, 0.02, 0.2), 100.0, start);
    const auto engine = make_monte_carlo_vanilla_engine({513, 50, 42}).value();
    for (const auto type : {OptionType::call, OptionType::put}) {
        const auto option = *make_european_option(type, 100.0, start, end);
        const auto scalar = engine.price(option, context);
        REQUIRE(scalar);
        for (const auto greek : {Greek::delta, Greek::gamma, Greek::speed, Greek::charm, Greek::color,
                                 Greek::vega, Greek::vanna, Greek::zomma, Greek::rho}) {
            const auto shifts = greek == Greek::rho                                                      ? NumericalShiftSettings{.rate_shift = 1e307}
                                : greek == Greek::vega || greek == Greek::vanna || greek == Greek::zomma ? NumericalShiftSettings{.volatility_shift = 0.3}
                                : greek == Greek::speed                                                  ? NumericalShiftSettings{.spot_shift = 60.0}
                                                                                                         : NumericalShiftSettings{.spot_shift = 200.0};
            const auto result = engine.price_with_greeks(option, context, {greek}, shifts);
            REQUIRE(result);
            CHECK(result->price() == *scalar);
            CHECK_FALSE(result->has(greek));
        }
        const GreeksRequest request{Greek::delta, Greek::gamma, Greek::speed, Greek::charm, Greek::color,
                                    Greek::vega, Greek::vanna, Greek::zomma, Greek::rho};
        const auto result = engine.price_with_greeks(option, context, request, {.spot_shift = 200.0, .volatility_shift = 0.3, .rate_shift = 1e307});
        REQUIRE(result);
        CHECK(result->price() == *scalar);
        CHECK(result->values_view() == make_pricing_result(*scalar)->values_view());
        const auto mixed = engine.price_with_greeks(option, context, {Greek::delta, Greek::vega}, {.spot_shift = 200.0});
        const auto expected = detail::price_with_greeks(engine, option, context, {Greek::delta, Greek::vega}, {.spot_shift = 200.0});
        REQUIRE(mixed);
        REQUIRE(expected);
        CHECK(mixed->values_view() == expected->values_view());
    }
}

TEST_CASE("American Monte Carlo unavailable stencils retain scalar prices", "[mc-american-samples]")
{
    using namespace kiyosi;
    const auto start = test::day(2025, 1, 1), end = test::day(2026, 1, 1);
    const auto context = *make_pricing_context(*make_bsm_parameters(0.05, 0.02, 0.2), 100.0, start);
    const auto engine = make_monte_carlo_vanilla_engine({127, 7, 42}).value();
    for (const auto type : {OptionType::call, OptionType::put}) {
        const auto option = *make_american_option(type, 100.0, start, end);
        const auto scalar = engine.price(option, context);
        REQUIRE(scalar);
        for (const auto greek : {Greek::delta, Greek::gamma, Greek::speed, Greek::vanna, Greek::zomma}) {
            const auto shifts = greek == Greek::vanna || greek == Greek::zomma
                                    ? NumericalShiftSettings{.volatility_shift = 0.3}
                                    : NumericalShiftSettings{.spot_shift = 200.0};
            const auto result = engine.price_with_greeks(option, context, {greek}, shifts);
            REQUIRE(result);
            CHECK(result->price() == *scalar);
            CHECK_FALSE(result->has(greek));
        }
    }
}

TEST_CASE("American Monte Carlo shared growth retains scalar Greek stencils", "[mc-american-samples]")
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1), end = day(2026, 1, 1);
    for (const auto type : {OptionType::call, OptionType::put})
        for (const double rate : {-0.02, 0.05})
            for (const double dividend : {0.0, 0.02})
                for (const int steps : {3, 7, 50})
                    for (const int paths : {3, 128})
                        for (const auto seed : {std::uint64_t{42}, std::numeric_limits<std::uint64_t>::max()}) {
                            CAPTURE(type, rate, dividend, steps, paths, seed);
                            const auto option = *make_american_option(type, 100.0, start, end);
                            const auto engine = make_monte_carlo_vanilla_engine({paths, steps, seed}).value();
                            for (const auto valuation : {start_of_day(start) + std::chrono::hours{12}, start_of_day(end)})
                                for (const auto shifts : {NumericalShiftSettings{}, NumericalShiftSettings{0.125, 0.02, 0.005, 2}}) {
                                    const auto context = *make_pricing_context(*make_bsm_parameters(rate, dividend, 0.2), 100.25, valuation);
                                    const auto expected = detail::price_with_greeks(engine, option, context, true, shifts);
                                    REQUIRE(expected);
                                    for (const GreeksRequest request : {GreeksRequest{true}, GreeksRequest{Greek::delta, Greek::gamma},
                                                                        GreeksRequest{Greek::speed}, GreeksRequest{Greek::vanna, Greek::zomma}}) {
                                        const auto actual = engine.price_with_greeks(option, context, request, shifts);
                                        REQUIRE(actual);
                                        CHECK(actual->price() == expected->price());
                                        CHECK(actual->values_view() == expected->selected(request).values_view());
                                    }
                                }
                        }
}

TEST_CASE("American Monte Carlo shared growth preserves failures and concurrent calls", "[mc-american-samples]")
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1), end = day(2026, 1, 1);
    const auto option = *make_american_option(OptionType::put, 100.0, start, end);
    const auto context = *make_pricing_context(*make_bsm_parameters(0.05, 0.02, 0.2), 100.25, start);
    const auto engine = make_monte_carlo_vanilla_engine({127, 7, 42}).value();
    for (const auto [volatility, rate_shift] : {std::pair{1000.0, 0.01}, std::pair{0.2, 1000.0}}) {
        const auto invalid = *make_pricing_context(*make_bsm_parameters(0.05, 0.02, volatility), 100.25, start);
        const NumericalShiftSettings shifts{.rate_shift = rate_shift};
        const auto expected = detail::price_with_greeks(engine, option, invalid, true, shifts);
        const auto actual = engine.price_with_greeks(option, invalid, true, shifts);
        REQUIRE_FALSE(expected);
        REQUIRE_FALSE(actual);
        CHECK(actual.error().category == expected.error().category);
    }
    const auto expected = engine.price_with_greeks(option, context, true);
    REQUIRE(expected);
    std::array<std::future<Result<PricingResult>>, 2> concurrent;
    for (auto& task : concurrent)
        task = std::async(std::launch::async, [&] { return engine.price_with_greeks(option, context, true); });
    for (auto& task : concurrent) {
        const auto actual = task.get();
        REQUIRE(actual);
        CHECK(actual->price() == expected->price());
        CHECK(actual->values_view() == expected->values_view());
    }
    const auto unseeded = make_monte_carlo_vanilla_engine({127, 7}).value();
    const auto random = unseeded.price_with_greeks(option, context, true);
    REQUIRE(random);
    CHECK(random->all_finite());
    CHECK_FALSE(unseeded.settings().seed);
}

TEST_CASE("American Monte Carlo skips exercise grids only when early exercise has no value", "[mc-terminal]")
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1), end = day(2026, 1, 1);
    const auto american = *make_american_option(OptionType::call, 100.0, start, end);
    const auto european = *make_european_option(OptionType::call, 100.0, start, end);
    for (const auto backend : {MonteCarloBackend::cpu
#if KIYOSI_HAS_CUDA
                               ,
                               MonteCarloBackend::cuda
#endif
         }) {
        for (const double rate : {0.0, 0.05}) {
            for (const double dividend : {-0.02, 0.0}) {
                const auto context = *make_pricing_context(*make_bsm_parameters(rate, dividend, 0.2), 100.0, start);
                const auto expected = make_monte_carlo_vanilla_engine({20'000, 2, 42, backend}).value().price(european, context);
                REQUIRE(expected);
                CHECK_THAT(*expected, Catch::Matchers::WithinAbs(*AnalyticVanillaEngine{}.price(european, context), 0.5));
                for (const int steps : {3, 50, 10'000}) {
                    CAPTURE(backend, rate, dividend, steps);
                    const auto engine = make_monte_carlo_vanilla_engine({19'999, steps, 42, backend}).value();
                    const auto actual = engine.price(american, context);
                    REQUIRE(actual);
                    CHECK(*actual == *expected);
                    CHECK(engine.settings().step_count == steps);
                }
            }
        }
        const auto deep = *make_pricing_context(*make_bsm_parameters(0.0, 0.0, 0.2), 2000.0, start);
        for (const auto seed : {1, 42, 73}) {
            const auto engine = make_monte_carlo_vanilla_engine({1, 3, seed, backend}).value();
            const auto continuation = engine.price(european, deep);
            const auto actual = engine.price(american, deep);
            REQUIRE(continuation);
            REQUIRE(actual);
            CHECK(*actual == std::max(*continuation, 1900.0));
        }
    }
    for (const auto& [type, rate, dividend, spot, expected] : {
             std::tuple{OptionType::call, -0.1, 0.0, 200.0, 100.0},
             std::tuple{OptionType::call, 0.0, 0.1, 200.0, 100.0},
             std::tuple{OptionType::put, 0.1, 0.0, 50.0, 50.0}}) {
        const auto context = *make_pricing_context(*make_bsm_parameters(rate, dividend, 1e-12), spot, start);
        const auto option = *make_american_option(type, 100.0, start, end);
        const auto actual = make_monte_carlo_vanilla_engine({64, 7, 42}).value().price(option, context);
        REQUIRE(actual);
        CHECK_THAT(*actual, Catch::Matchers::WithinAbs(expected, 1e-10));
    }
    const auto context = *make_pricing_context(*make_bsm_parameters(0.05, 0.0, 0.2), 110.0, start);
    for (const auto backend : {MonteCarloBackend::cpu, MonteCarloBackend::cuda}) {
        for (const auto settings : {MonteCarloSettings{0, 50, 42, backend}, MonteCarloSettings{16, 2, 42, backend}, MonteCarloSettings{16, 10'001, 42, backend}}) {
            const auto engine = make_monte_carlo_vanilla_engine(settings);
            if (settings.path_count == 0 || settings.step_count > 10'000) {
                REQUIRE_FALSE(engine);
                CHECK(engine.error().category == ErrorCategory::invalid_parameter);
            } else {
                REQUIRE(engine);
                const auto invalid = engine->price(american, context);
                REQUIRE_FALSE(invalid);
                CHECK(invalid.error().category == ErrorCategory::invalid_parameter);
            }
        }
        const auto expired = *make_pricing_context(context.model_parameters(), 110.0, end);
        CHECK(*make_monte_carlo_vanilla_engine({16, 2, 42, backend}).value().price(american, expired) == 10.0);
    }
}

TEST_CASE("American Monte Carlo rate Greeks retain one estimator across zero", "[mc-terminal]")
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1), end = day(2026, 1, 1);
    const auto option = *make_american_option(OptionType::call, 100.0, start, end);
    const auto engine = make_monte_carlo_vanilla_engine({20'000, 7, 42}).value();
    NumericalShiftSettings shifts;
    shifts.rate_shift = 1.0 / 8192.0;
    for (const double rate : {-1.0 / 16384.0, 0.0, 1.0 / 16384.0}) {
        CAPTURE(rate);
        const auto context = *make_pricing_context(*make_bsm_parameters(rate, 0.0, 0.2), 100.0, start);
        const auto rho = engine.price_with_greeks(option, context, {Greek::rho}, shifts);
        const auto all = kiyosi::test::price_differences(engine, option, context, shifts);
        REQUIRE(rho);
        REQUIRE(all);
        CHECK(rho->price() == *engine.price(option, context));
        CHECK(all->price() == rho->price());
        REQUIRE(rho->require(Greek::rho));
        REQUIRE(all->require(Greek::rho));
        const double shift = shifts.rate_shift;
        // A subnormal dividend leaves coefficients unchanged while retaining the exercise grid.
        const auto exercise_value = [&](double bumped_rate) {
            const auto shifted = *make_pricing_context(*make_bsm_parameters(bumped_rate, std::numeric_limits<double>::denorm_min(), 0.2), 100.0, start);
            const auto value = engine.price(option, shifted);
            REQUIRE(value);
            return *value;
        };
        const double expected = (exercise_value(rate + shift) - exercise_value(rate - shift)) / (200.0 * shift);
        CHECK_THAT(*rho->require(Greek::rho), Catch::Matchers::WithinAbs(expected, 1e-10));
        CHECK(*all->require(Greek::rho) == *rho->require(Greek::rho));
        const auto spot = engine.price_with_greeks(option, context, {Greek::delta, Greek::gamma}, shifts);
        REQUIRE(spot);
        CHECK(all->get(Greek::delta) == spot->get(Greek::delta));
        CHECK(all->get(Greek::gamma) == spot->get(Greek::gamma));
    }
}

TEST_CASE("American Monte Carlo default spot Greeks converge across seeds and price scales", "[mc-regression]")
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1), end = day(2026, 1, 1);
    const auto model = *make_bsm_parameters(0.05, 0.02, 0.2);
    const auto reference_option = *make_american_option(OptionType::put, 100.0, start, end);
    const auto reference_context = *make_pricing_context(model, 100.0, start);
    const auto reference = make_finite_difference_vanilla_engine({1200, 1200}).value().price_with_greeks(reference_option, reference_context, {Greek::delta, Greek::gamma});
    REQUIRE(reference);
    for (const double scale : {0.01, 1.0, 100.0}) {
        const auto option = *make_american_option(OptionType::put, 100.0 * scale, start, end);
        const auto context = *make_pricing_context(model, 100.0 * scale, start);
        for (const std::uint64_t seed : {1, 7, 42}) {
            CAPTURE(scale, seed);
            const auto engine = make_monte_carlo_vanilla_engine({100'000, 50, seed}).value();
            const auto result = engine.price_with_greeks(option, context, {Greek::delta, Greek::gamma});
            REQUIRE(result);
            CHECK_THAT(*result->require(Greek::delta), Catch::Matchers::WithinAbs(*reference->require(Greek::delta), 0.02));
            CHECK_THAT(*result->require(Greek::gamma) * scale, Catch::Matchers::WithinAbs(*reference->require(Greek::gamma), 0.004));
            const auto explicit_result = engine.price_with_greeks(option, context, {Greek::delta, Greek::gamma},
                                                                  {.spot_shift = 8.0 * scale});
            REQUIRE(explicit_result);
            CHECK(result->values_view() == explicit_result->values_view());
        }
    }
}

TEST_CASE("American Monte Carlo preserves paired paths across time grids", "[monte-carlo-performance][mc-regression]")
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1), end = day(2026, 1, 1);
    const auto context = *make_pricing_context(*make_bsm_parameters(0.05, 0.02, 0.2), 100.0, start);
    for (const auto type : {OptionType::call, OptionType::put}) {
        const auto option = *make_american_option(type, 100.0, start, end);
        for (const int steps : {3, 7, 50}) {
            for (const int paths : {1, 3, 19'999}) {
                CAPTURE(type, steps, paths);
                const auto odd = make_monte_carlo_vanilla_engine({paths, steps, 42}).value().price(option, context);
                const auto even = make_monte_carlo_vanilla_engine({paths + 1, steps, 42}).value().price(option, context);
                REQUIRE(odd);
                REQUIRE(even);
                CHECK(*odd == *even);
            }
        }
    }
}

TEST_CASE("American Monte Carlo includes immediate exercise in the exercise window")
{
    const auto valuation = day(2025, 1, 6);
    const auto expiry_date = day(2026, 1, 6);
    const auto parameters = *kiyosi::make_bsm_parameters(0.10, 0.0, 0.10);
    const auto context = *kiyosi::make_pricing_context(parameters, 50.0, valuation);
    const auto put = *kiyosi::make_american_option(kiyosi::OptionType::put, 100.0, valuation, expiry_date);
    const auto result = kiyosi::make_monte_carlo_vanilla_engine({20'000, 50, 42}).value().price(put, context);
    REQUIRE(result.has_value());
    CHECK_THAT(*result, Catch::Matchers::WithinAbs(50.0, 1e-10));
}

TEST_CASE("American Monte Carlo preserves sparse and singular regression fallbacks", "[mc-regression]")
{
    const auto valuation = day(2025, 1, 1);
    const auto expiry_date = valuation + std::chrono::days{365};
    const auto parameters = *kiyosi::make_bsm_parameters(
        0.0, 0.10, std::numeric_limits<double>::min());
    const auto context = *kiyosi::make_pricing_context(parameters, 50.0, valuation);
    const auto put = *kiyosi::make_american_option(
        kiyosi::OptionType::put, 100.0, valuation, expiry_date);

    const auto sparse = kiyosi::make_monte_carlo_vanilla_engine({1, 5, 42}).value().price(put, context);
    const auto singular = kiyosi::make_monte_carlo_vanilla_engine({4, 5, 42}).value().price(put, context);

    REQUIRE(sparse);
    REQUIRE(singular);
    CHECK(*sparse ==
          *singular);
    CHECK(*sparse > 50.0);
}

TEST_CASE("Monte Carlo defaults to CPU and preserves explicit CPU pricing")
{
    const auto valuation = day(2025, 1, 1);
    const auto expiry_date = valuation + std::chrono::days{365};
    const auto parameters = *kiyosi::make_bsm_parameters(0.05, 0.02, 0.2);
    const auto context = *kiyosi::make_pricing_context(parameters, 100.0, valuation);
    const auto call = *kiyosi::make_european_option(
        kiyosi::OptionType::call, 100.0, valuation, expiry_date);
    const auto implicit_cpu = kiyosi::make_monte_carlo_vanilla_engine({20'000, 50, 42}).value();
    const auto explicit_cpu = kiyosi::make_monte_carlo_vanilla_engine({20'000, 50, 42, kiyosi::MonteCarloBackend::cpu}).value();

    CHECK(implicit_cpu.settings().backend == kiyosi::MonteCarloBackend::cpu);
    const auto implicit_result = implicit_cpu.price(call, context);
    const auto explicit_result = explicit_cpu.price(call, context);
    REQUIRE(implicit_result);
    REQUIRE(explicit_result);
    CHECK(*implicit_result ==
          *explicit_result);

    auto invalid_settings = implicit_cpu.settings();
    invalid_settings.backend = static_cast<kiyosi::MonteCarloBackend>(255);
    const auto invalid = kiyosi::make_monte_carlo_vanilla_engine(invalid_settings);
    REQUIRE_FALSE(invalid);
    CHECK(invalid.error().category == kiyosi::ErrorCategory::invalid_parameter);
}

#if !KIYOSI_HAS_CUDA
TEST_CASE("CUDA selection is deferred and unavailable builds do not fall back")
{
    const auto valuation = day(2025, 1, 1);
    const auto expiry_date = valuation + std::chrono::days{365};
    const auto parameters = *kiyosi::make_bsm_parameters(0.05, 0.02, 0.2);
    const auto context = *kiyosi::make_pricing_context(parameters, 100.0, valuation);
    const auto call = *kiyosi::make_european_option(
        kiyosi::OptionType::call, 100.0, valuation, expiry_date);
    const auto engine = kiyosi::make_monte_carlo_vanilla_engine({20'000, 50, 42, kiyosi::MonteCarloBackend::cuda}).value();

    CHECK(engine.settings().backend == kiyosi::MonteCarloBackend::cuda);
    const auto result = engine.price(call, context);
    REQUIRE_FALSE(result);
    CHECK(result.error().category == kiyosi::ErrorCategory::backend_unavailable);
}
#endif

#if !KIYOSI_HAS_CUDA
TEST_CASE("CUDA American selection is deferred and unavailable builds do not fall back")
{
    const auto valuation = day(2025, 1, 1);
    const auto expiry_date = valuation + std::chrono::days{365};
    const auto parameters = *kiyosi::make_bsm_parameters(0.05, 0.02, 0.2);
    const auto context = *kiyosi::make_pricing_context(parameters, 100.0, valuation);
    const auto put = *kiyosi::make_american_option(
        kiyosi::OptionType::put, 100.0, valuation, expiry_date);
    const auto engine = kiyosi::make_monte_carlo_vanilla_engine({20'000, 50, 42, kiyosi::MonteCarloBackend::cuda}).value();

    const auto result = engine.price(put, context);
    REQUIRE_FALSE(result);
    CHECK(result.error().category == kiyosi::ErrorCategory::backend_unavailable);
}

TEST_CASE("American Monte Carlo validates settings before CUDA availability")
{
    const auto valuation = day(2025, 1, 1);
    const auto expiry_date = valuation + std::chrono::days{365};
    const auto parameters = *kiyosi::make_bsm_parameters(0.05, 0.02, 0.2);
    const auto context = *kiyosi::make_pricing_context(parameters, 100.0, valuation);
    const auto put = *kiyosi::make_american_option(
        kiyosi::OptionType::put, 100.0, valuation, expiry_date);
    constexpr std::array invalid_settings{
        kiyosi::MonteCarloSettings{0, 50, 42, kiyosi::MonteCarloBackend::cuda},
        kiyosi::MonteCarloSettings{20, 2, 42, kiyosi::MonteCarloBackend::cuda},
    };

    for (const auto settings : invalid_settings) {
        CAPTURE(settings.path_count, settings.step_count);
        const auto engine = kiyosi::make_monte_carlo_vanilla_engine(settings);
        if (settings.path_count == 0) {
            REQUIRE_FALSE(engine);
            CHECK(engine.error().category == kiyosi::ErrorCategory::invalid_parameter);
        } else {
            REQUIRE(engine);
            const auto result = engine->price(put, context);
            REQUIRE_FALSE(result);
            CHECK(result.error().category == kiyosi::ErrorCategory::invalid_parameter);
        }
    }
}
#endif

TEST_CASE("CUDA American Monte Carlo returns intrinsic value at expiry_date without a backend")
{
    struct Case {
        kiyosi::OptionType type;
        double spot;
        double expected;
    };
    constexpr std::array cases{
        Case{kiyosi::OptionType::call, 110.0, 10.0},
        Case{kiyosi::OptionType::call, 90.0, 0.0},
        Case{kiyosi::OptionType::put, 90.0, 10.0},
        Case{kiyosi::OptionType::put, 110.0, 0.0},
    };
    const auto expiry_date = day(2026, 1, 1);
    const auto parameters = *kiyosi::make_bsm_parameters(0.05, 0.02, 0.2);

    for (const auto test : cases) {
        CAPTURE(test.type, test.spot);
        const auto context = *kiyosi::make_pricing_context(parameters, test.spot, expiry_date);
        const auto option = *kiyosi::make_american_option(
            test.type, 100.0, expiry_date, expiry_date);
        const auto result = kiyosi::make_monte_carlo_vanilla_engine({20, 3, 42, kiyosi::MonteCarloBackend::cuda}).value().price(option, context);
        REQUIRE(result);
        CHECK(*result == test.expected);
    }
}

#if KIYOSI_HAS_CUDA
TEST_CASE("CUDA pricing preserves the caller's current device", "[cuda]")
{
    using namespace kiyosi;
    int original_device = 0;
    REQUIRE(cudaGetDevice(&original_device) == cudaSuccess);
    struct RestoreDevice {
        int device;
        explicit RestoreDevice(int value) : device{value} {}
        RestoreDevice(const RestoreDevice&) = delete;
        RestoreDevice& operator=(const RestoreDevice&) = delete;
        RestoreDevice(RestoreDevice&&) = delete;
        RestoreDevice& operator=(RestoreDevice&&) = delete;
        ~RestoreDevice() { cudaSetDevice(device); }
    };
    const RestoreDevice restore{original_device};
    int device_count = 0;
    REQUIRE(cudaGetDeviceCount(&device_count) == cudaSuccess);
    REQUIRE(device_count > 0);
    const auto start = day(2025, 1, 1);
    const auto end = day(2026, 1, 1);
    const auto context = *make_pricing_context(*make_bsm_parameters(0.05, 0.02, 0.2), 100.0, start);
    const auto european = *make_european_option(OptionType::call, 100.0, start, end);
    const auto american = *make_american_option(OptionType::put, 100.0, start, end);
    const auto accumulator = *make_accumulator({.strike = 100.0, .knock_out_level = 120.0, .daily_quantity = 1.0, .acceleration_factor = 2.0, .effective_date = start, .expiry_date = end});
    const auto note = *make_binary_snowball_option({.maturity_coupon_rate = 0.1, .effective_date = start, .expiry_date = end, .observations = {{end, 120.0, 0.1}}});
    const auto vanilla = make_monte_carlo_vanilla_engine({64, 4, 42, MonteCarloBackend::cuda}).value();
    const auto accrual = make_monte_carlo_accumulator_engine({64, 42, MonteCarloBackend::cuda}).value();
    const auto autocallable = make_monte_carlo_autocallable_engine({64, 42, MonteCarloBackend::cuda}).value();
    const auto unstable = *make_pricing_context(*make_bsm_parameters(0.0, 0.0, 1000.0), 100.0, start);
    for (int device = 0; device < device_count; ++device) {
        CAPTURE(device);
        REQUIRE(cudaSetDevice(device) == cudaSuccess);
        const auto check_device = [&] {
            int current_device = -1;
            REQUIRE(cudaGetDevice(&current_device) == cudaSuccess);
            CHECK(current_device == device);
        };
        REQUIRE(vanilla.price(european, context));
        check_device();
        REQUIRE(vanilla.price(american, context));
        check_device();
        REQUIRE(accrual.price(accumulator, context));
        check_device();
        REQUIRE(autocallable.price(note, context));
        check_device();
        const auto failed = vanilla.price(european, unstable);
        REQUIRE_FALSE(failed);
        CHECK(failed.error().category == ErrorCategory::invalid_result);
        check_device();
    }
}

TEST_CASE("CUDA American Monte Carlo prices are invariant to currency scale", "[cuda]")
{
    check_american_currency_scale(kiyosi::MonteCarloBackend::cuda);
}

TEST_CASE("CUDA European Monte Carlo is seeded and deterministic", "[cuda]")
{
    const auto valuation = day(2025, 1, 1);
    const auto expiry_date = valuation + std::chrono::days{365};
    const auto parameters = *kiyosi::make_bsm_parameters(0.05, 0.02, 0.2);
    const auto context = *kiyosi::make_pricing_context(parameters, 100.0, valuation);
    const auto call = *kiyosi::make_european_option(
        kiyosi::OptionType::call, 100.0, valuation, expiry_date);
    const auto engine = kiyosi::make_monte_carlo_vanilla_engine({100'000, 50, 42, kiyosi::MonteCarloBackend::cuda}).value();

    const auto first = engine.price(call, context);
    const auto second = engine.price(call, context);
    const auto different_seed = kiyosi::make_monte_carlo_vanilla_engine({100'000, 50, 43, kiyosi::MonteCarloBackend::cuda}).value().price(call, context);
    const auto repeated_different_seed = kiyosi::make_monte_carlo_vanilla_engine({100'000, 50, 43, kiyosi::MonteCarloBackend::cuda}).value().price(call, context);
    const auto different_path_count = kiyosi::make_monte_carlo_vanilla_engine({10'000, 50, 42, kiyosi::MonteCarloBackend::cuda}).value().price(call, context);
    REQUIRE(first);
    REQUIRE(second);
    REQUIRE(different_seed);
    REQUIRE(repeated_different_seed);
    REQUIRE(different_path_count);
    CHECK(*first ==
          *second);
    CHECK(*different_seed ==
          *repeated_different_seed);
    CHECK(*first !=
          *different_seed);
    CHECK(*first !=
          *different_path_count);
    CHECK(*first >= 0.0);
}

TEST_CASE("CUDA European Monte Carlo agrees with CPU and analytic prices", "[cuda]")
{
    struct Case {
        kiyosi::OptionType type;
        double strike;
    };
    constexpr std::array cases{
        Case{kiyosi::OptionType::call, 90.0},
        Case{kiyosi::OptionType::call, 100.0},
        Case{kiyosi::OptionType::call, 110.0},
        Case{kiyosi::OptionType::put, 100.0},
    };
    constexpr double tolerance = 0.35;
    const auto valuation = day(2025, 1, 1);
    const auto expiry_date = valuation + std::chrono::days{365};
    const auto parameters = *kiyosi::make_bsm_parameters(0.05, 0.02, 0.2);
    const auto context = *kiyosi::make_pricing_context(parameters, 100.0, valuation);

    for (const auto test : cases) {
        CAPTURE(test.type, test.strike);
        const auto option = *kiyosi::make_european_option(
            test.type, test.strike, valuation, expiry_date);
        const auto cpu = kiyosi::make_monte_carlo_vanilla_engine({100'000, 50, 42, kiyosi::MonteCarloBackend::cpu}).value().price(option, context);
        const auto cuda = kiyosi::make_monte_carlo_vanilla_engine({100'000, 50, 42, kiyosi::MonteCarloBackend::cuda}).value().price(option, context);
        const auto analytic = kiyosi::AnalyticVanillaEngine{}.price(option, context);
        REQUIRE(cpu);
        REQUIRE(cuda);
        REQUIRE(analytic);
        const double cuda_price = *cuda;
        CHECK_THAT(cuda_price, Catch::Matchers::WithinAbs(
                                   *cpu, tolerance));
        CHECK_THAT(cuda_price, Catch::Matchers::WithinAbs(
                                   *analytic, tolerance));
    }
}

TEST_CASE("CUDA European Monte Carlo rounds odd path counts for antithetic pairs", "[cuda]")
{
    const auto valuation = day(2025, 1, 1);
    const auto expiry_date = valuation + std::chrono::days{365};
    const auto parameters = *kiyosi::make_bsm_parameters(0.05, 0.02, 0.2);
    const auto context = *kiyosi::make_pricing_context(parameters, 100.0, valuation);
    const auto call = *kiyosi::make_european_option(
        kiyosi::OptionType::call, 100.0, valuation, expiry_date);

    const auto baseline = kiyosi::make_monte_carlo_vanilla_engine({10'000, 2, 42, kiyosi::MonteCarloBackend::cuda}).value().price(call, context);
    REQUIRE(baseline);
    for (const int step_count : {2, 7, 50, 10'000}) {
        CAPTURE(step_count);
        const auto odd = kiyosi::make_monte_carlo_vanilla_engine({9'999, step_count, 42, kiyosi::MonteCarloBackend::cuda}).value().price(call, context);
        const auto even = kiyosi::make_monte_carlo_vanilla_engine({10'000, step_count, 42, kiyosi::MonteCarloBackend::cuda}).value().price(call, context);
        REQUIRE(odd);
        REQUIRE(even);
        CHECK(*odd ==
              *even);
        CHECK(*even == *baseline);
    }
}

TEST_CASE("CUDA Monte Carlo supports concurrent const pricing", "[cuda]")
{
    const auto valuation = day(2025, 1, 1);
    const auto expiry_date = valuation + std::chrono::days{365};
    const auto parameters = *kiyosi::make_bsm_parameters(0.05, 0.02, 0.2);
    const auto context = *kiyosi::make_pricing_context(parameters, 100.0, valuation);
    const auto call = *kiyosi::make_european_option(
        kiyosi::OptionType::call, 100.0, valuation, expiry_date);
    const auto engine = kiyosi::make_monte_carlo_vanilla_engine({20'000, 50, 42, kiyosi::MonteCarloBackend::cuda}).value();
    const auto baseline = engine.price(call, context);
    REQUIRE(baseline);

    std::array<std::future<kiyosi::Result<double>>, 4> results;
    for (auto& result : results)
        result = std::async(std::launch::async, [&] { return engine.price(call, context); });
    for (auto& pending : results) {
        const auto result = pending.get();
        REQUIRE(result);
        CHECK(*result ==
              *baseline);
    }
}

TEST_CASE("CUDA American Monte Carlo is seeded and deterministic", "[cuda]")
{
    struct Case {
        kiyosi::OptionType type;
        double rate;
        double dividend;
    };
    constexpr std::array cases{
        Case{kiyosi::OptionType::put, 0.05, 0.0},
        Case{kiyosi::OptionType::call, 0.02, 0.08},
    };
    const auto valuation = day(2025, 1, 1);
    const auto expiry_date = valuation + std::chrono::days{365};

    for (const auto test : cases) {
        CAPTURE(test.type, test.rate, test.dividend);
        const auto parameters = *kiyosi::make_bsm_parameters(
            test.rate, test.dividend, 0.2);
        const auto context = *kiyosi::make_pricing_context(parameters, 100.0, valuation);
        const auto option = *kiyosi::make_american_option(
            test.type, 100.0, valuation, expiry_date);
        const auto engine = kiyosi::make_monte_carlo_vanilla_engine({50'000, 50, 42, kiyosi::MonteCarloBackend::cuda}).value();
        const auto first = engine.price(option, context);
        const auto second = engine.price(option, context);
        REQUIRE(first);
        REQUIRE(second);
        CHECK(*first ==
              *second);
    }

    const auto parameters = *kiyosi::make_bsm_parameters(0.05, 0.0, 0.2);
    const auto context = *kiyosi::make_pricing_context(parameters, 100.0, valuation);
    const auto put = *kiyosi::make_american_option(
        kiyosi::OptionType::put, 100.0, valuation, expiry_date);
    const auto baseline = kiyosi::make_monte_carlo_vanilla_engine({50'000, 50, 42, kiyosi::MonteCarloBackend::cuda}).value().price(put, context);
    const auto different_seed = kiyosi::make_monte_carlo_vanilla_engine({50'000, 50, 43, kiyosi::MonteCarloBackend::cuda}).value().price(put, context);
    const auto different_path_count = kiyosi::make_monte_carlo_vanilla_engine({40'000, 50, 42, kiyosi::MonteCarloBackend::cuda}).value().price(put, context);
    const auto cpu = kiyosi::make_monte_carlo_vanilla_engine({50'000, 50, 42, kiyosi::MonteCarloBackend::cpu}).value().price(put, context);
    REQUIRE(baseline);
    REQUIRE(different_seed);
    REQUIRE(different_path_count);
    REQUIRE(cpu);
    CHECK(*baseline !=
          *different_seed);
    CHECK(*baseline !=
          *different_path_count);
    CHECK(*baseline !=
          *cpu);
}

TEST_CASE("CUDA American Monte Carlo prices early exercise for puts and dividend calls", "[cuda]")
{
    struct Case {
        kiyosi::OptionType type;
        double rate;
        double dividend;
        double spot;
    };
    constexpr std::array cases{
        Case{kiyosi::OptionType::put, 0.10, 0.0, 50.0},
        Case{kiyosi::OptionType::call, 0.0, 0.10, 150.0},
    };
    const auto valuation = day(2025, 1, 6);
    const auto expiry_date = day(2026, 1, 6);

    for (const auto test : cases) {
        CAPTURE(test.type, test.rate, test.dividend, test.spot);
        const auto parameters = *kiyosi::make_bsm_parameters(
            test.rate, test.dividend, 0.10);
        const auto context = *kiyosi::make_pricing_context(
            parameters, test.spot, valuation);
        const auto option = *kiyosi::make_american_option(
            test.type, 100.0, valuation, expiry_date);
        const auto result = kiyosi::make_monte_carlo_vanilla_engine({100'000, 50, 42, kiyosi::MonteCarloBackend::cuda}).value().price(option, context);
        REQUIRE(result);
        CHECK_THAT(*result,
                   Catch::Matchers::WithinAbs(50.0, 1e-10));
    }
}

TEST_CASE("CUDA American Monte Carlo agrees with CPU and finite-difference prices", "[cuda]")
{
    struct Case {
        kiyosi::OptionType type;
        double rate;
        double dividend;
    };
    constexpr std::array cases{
        Case{kiyosi::OptionType::put, 0.05, 0.0},
        Case{kiyosi::OptionType::call, 0.02, 0.08},
    };
    constexpr double tolerance = 0.5;
    const auto valuation = day(2025, 1, 1);
    const auto expiry_date = valuation + std::chrono::days{365};

    for (const auto test : cases) {
        CAPTURE(test.type, test.rate, test.dividend);
        const auto parameters = *kiyosi::make_bsm_parameters(
            test.rate, test.dividend, 0.2);
        const auto context = *kiyosi::make_pricing_context(parameters, 100.0, valuation);
        const auto option = *kiyosi::make_american_option(
            test.type, 100.0, valuation, expiry_date);
        const auto european_option = *kiyosi::make_european_option(
            test.type, 100.0, valuation, expiry_date);
        const auto cpu = kiyosi::make_monte_carlo_vanilla_engine({200'000, 50, 42, kiyosi::MonteCarloBackend::cpu}).value().price(option, context);
        const auto cuda = kiyosi::make_monte_carlo_vanilla_engine({200'000, 50, 42, kiyosi::MonteCarloBackend::cuda}).value().price(option, context);
        const auto european_cuda = kiyosi::make_monte_carlo_vanilla_engine({200'000, 50, 42, kiyosi::MonteCarloBackend::cuda}).value().price(european_option, context);
        const auto finite_difference = kiyosi::make_finite_difference_vanilla_engine({400, 800, kiyosi::FiniteDifferenceScheme::crank_nicolson}).value().price(option, context);
        REQUIRE(cpu);
        REQUIRE(cuda);
        REQUIRE(european_cuda);
        REQUIRE(finite_difference);
        const double cuda_price = *cuda;
        CHECK(cuda_price > *european_cuda);
        CHECK_THAT(cuda_price, Catch::Matchers::WithinAbs(
                                   *cpu, tolerance));
        CHECK_THAT(cuda_price, Catch::Matchers::WithinAbs(
                                   *finite_difference,
                                   tolerance));
    }
}

TEST_CASE("CUDA American Monte Carlo rounds odd path counts for antithetic pairs", "[cuda]")
{
    const auto valuation = day(2025, 1, 1);
    const auto expiry_date = valuation + std::chrono::days{365};
    const auto parameters = *kiyosi::make_bsm_parameters(0.05, 0.0, 0.2);
    const auto context = *kiyosi::make_pricing_context(parameters, 100.0, valuation);
    const auto put = *kiyosi::make_american_option(
        kiyosi::OptionType::put, 100.0, valuation, expiry_date);
    const auto odd = kiyosi::make_monte_carlo_vanilla_engine({9'999, 50, 42, kiyosi::MonteCarloBackend::cuda}).value().price(put, context);
    const auto even = kiyosi::make_monte_carlo_vanilla_engine({10'000, 50, 42, kiyosi::MonteCarloBackend::cuda}).value().price(put, context);

    REQUIRE(odd);
    REQUIRE(even);
    CHECK(*odd ==
          *even);
}

TEST_CASE("CUDA American Monte Carlo preserves sparse and singular regression fallbacks", "[cuda]")
{
    const auto valuation = day(2025, 1, 1);
    const auto expiry_date = valuation + std::chrono::days{365};
    const auto parameters = *kiyosi::make_bsm_parameters(
        0.0, 0.10, std::numeric_limits<double>::min());
    const auto context = *kiyosi::make_pricing_context(parameters, 50.0, valuation);
    const auto put = *kiyosi::make_american_option(
        kiyosi::OptionType::put, 100.0, valuation, expiry_date);
    const auto sparse = kiyosi::make_monte_carlo_vanilla_engine({1, 5, 42, kiyosi::MonteCarloBackend::cuda}).value().price(put, context);
    const auto singular = kiyosi::make_monte_carlo_vanilla_engine({4, 5, 42, kiyosi::MonteCarloBackend::cuda}).value().price(put, context);
    const auto sparse_cpu = kiyosi::make_monte_carlo_vanilla_engine({1, 5, 42, kiyosi::MonteCarloBackend::cpu}).value().price(put, context);
    const auto singular_cpu = kiyosi::make_monte_carlo_vanilla_engine({4, 5, 42, kiyosi::MonteCarloBackend::cpu}).value().price(put, context);

    REQUIRE(sparse);
    REQUIRE(singular);
    REQUIRE(sparse_cpu);
    REQUIRE(singular_cpu);
    CHECK(*sparse ==
          *singular);
    CHECK_THAT(*sparse,
               Catch::Matchers::WithinAbs(
                   *sparse_cpu, 1e-10));
    CHECK_THAT(*singular,
               Catch::Matchers::WithinAbs(
                   *singular_cpu, 1e-10));
}

TEST_CASE("CUDA American Monte Carlo supports concurrent const pricing", "[cuda]")
{
    const auto valuation = day(2025, 1, 1);
    const auto expiry_date = valuation + std::chrono::days{365};
    const auto parameters = *kiyosi::make_bsm_parameters(0.05, 0.0, 0.2);
    const auto context = *kiyosi::make_pricing_context(parameters, 100.0, valuation);
    const auto put = *kiyosi::make_american_option(
        kiyosi::OptionType::put, 100.0, valuation, expiry_date);
    const auto engine = kiyosi::make_monte_carlo_vanilla_engine({20'000, 50, 42, kiyosi::MonteCarloBackend::cuda}).value();
    const auto baseline = engine.price(put, context);
    REQUIRE(baseline);

    std::array<std::future<kiyosi::Result<double>>, 4> results;
    for (auto& result : results)
        result = std::async(std::launch::async, [&] { return engine.price(put, context); });
    for (auto& pending : results) {
        const auto result = pending.get();
        REQUIRE(result);
        CHECK(*result ==
              *baseline);
    }
}

TEST_CASE("CUDA American Monte Carlo discounts every exercise interval", "[cuda]")
{
    const auto valuation = day(2025, 1, 1);
    const auto expiry_date = valuation + std::chrono::days{365};
    const auto parameters = *kiyosi::make_bsm_parameters(
        0.05, 0.0, std::numeric_limits<double>::min());
    const auto context = *kiyosi::make_pricing_context(parameters, 100.0, valuation);
    const auto call = *kiyosi::make_american_option(
        kiyosi::OptionType::call, 100.0, valuation, expiry_date);
    const auto result = kiyosi::make_monte_carlo_vanilla_engine({1, 3, 42, kiyosi::MonteCarloBackend::cuda}).value().price(call, context);

    REQUIRE(result);
    CHECK_THAT(*result,
               Catch::Matchers::WithinAbs(100.0 * (1.0 - std::exp(-0.05)), 1e-10));
}

TEST_CASE("CUDA American Monte Carlo reports non-finite paths", "[cuda]")
{
    const auto valuation = day(2025, 1, 1);
    const auto expiry_date = valuation + std::chrono::days{365};
    const auto parameters = *kiyosi::make_bsm_parameters(0.05, 0.0, 1'000.0);
    const auto context = *kiyosi::make_pricing_context(parameters, 100.0, valuation);
    const auto put = *kiyosi::make_american_option(
        kiyosi::OptionType::put, 100.0, valuation, expiry_date);
    const auto result = kiyosi::make_monte_carlo_vanilla_engine({20, 3, 42, kiyosi::MonteCarloBackend::cuda}).value().price(put, context);

    REQUIRE_FALSE(result);
    CHECK(result.error().category == kiyosi::ErrorCategory::invalid_result);
}
#endif

} // namespace
