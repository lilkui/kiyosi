#include <catch2/catch_test_macros.hpp>
#include <catch2/matchers/catch_matchers_floating_point.hpp>
#include <array>
#include <chrono>
#include <cmath>
#include <future>
#include <limits>
#include <string>
#include <type_traits>
#include <vector>
#include <kiyosi/kiyosi.hpp>
#include "support/common.hpp"
#include "pricing/engines/monte_carlo_cuda_host.hpp"
#include "pricing/engines/monte_carlo_mean.hpp"

#if KIYOSI_HAS_CUDA
#include <cuda_runtime_api.h>
#endif

TEST_CASE("Monte Carlo averages retain finite extreme and subnormal payoffs", "[audit-fixes]")
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
    REQUIRE_THROWS_AS(cuda_mean({CudaPricingStatus::out_of_memory, 0.0, nullptr}), std::bad_alloc);
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
    const MonteCarloVanillaEngine engine{20'000, 50, 47, backend};
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
    const MonteCarloVanillaEngine vanilla{100'000, 2, 42, backend};
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
    const MonteCarloAccumulatorEngine accumulator{{100'000, 42, backend}};
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
    const MonteCarloBinarySnowballEngine structured{{100'000, 42, backend}};
    for (const double principal : {1e305, std::numeric_limits<double>::denorm_min()}) {
        const auto option = *make_binary_snowball_option(
            {.knock_out_coupon_rates = {0.0}, .maturity_coupon_rate = 0.0, .knock_out_levels = {120.0}, .observation_dates = {next_day}, .principal_ratio = principal, .effective_date = start, .expiry_date = next_day});
        const auto price = structured.price(option, *make_pricing_context(parameters, 100.0, start));
        REQUIRE(price);
        CHECK(*price == principal);
    }
    const auto overflow = *make_binary_snowball_option(
        {.knock_out_coupon_rates = {1e308}, .maturity_coupon_rate = 1e308, .knock_out_levels = {120.0}, .observation_dates = {end}, .principal_ratio = 1e308, .effective_date = start, .expiry_date = end});
    const auto invalid = MonteCarloBinarySnowballEngine{{32, 42, backend}}.price(
        overflow, *make_pricing_context(parameters, 100.0, start));
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

TEST_CASE("American Monte Carlo prices are invariant to currency scale")
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

    const kiyosi::MonteCarloVanillaEngine european{20'000, 2, 42};
    const auto first = european.price(call, context);
    const auto second = european.price(call, context);
    REQUIRE(first.has_value());
    REQUIRE(second.has_value());
    CHECK(*first == *second);
    CHECK(std::abs(*first -
                   *kiyosi::AnalyticVanillaEngine{}.price(call, context)) < 0.5);

    const kiyosi::MonteCarloVanillaEngine american_engine{20'000, 20, 42};
    const auto american_result = american_engine.price(american, context);
    REQUIRE(american_result.has_value());
    CHECK(*american_result >= 0.0);

    CHECK_FALSE(kiyosi::MonteCarloVanillaEngine{0, 2}.price(call, context).has_value());
    CHECK_FALSE(kiyosi::MonteCarloVanillaEngine{20, 2}.price(american, context).has_value());
}

TEST_CASE("Vanilla Monte Carlo validates generic settings at expiry", "[audit-fixes]")
{
    using namespace kiyosi;
    const auto effective = day(2025, 1, 1);
    const auto expiry = day(2026, 1, 1);
    const auto parameters = *make_bsm_parameters(0.05, 0.02, 0.2);
    const auto check = [&](const auto& option) {
        for (const MonteCarloSettings settings : {
                 MonteCarloSettings{0, 2}, {-1, 2}, {10'000'001, 2}, {1, 0}, {1, 1}, {1, 10'001},
                 {1, 2, 1, static_cast<MonteCarloBackend>(255)}}) {
            const MonteCarloVanillaEngine engine{settings};
            for (const auto date : {effective, expiry}) {
                const auto market = *make_pricing_context(parameters, 110.0, date);
                const auto result = engine.price(option, market);
                REQUIRE_FALSE(result);
                CHECK(result.error().category == ErrorCategory::invalid_parameter);
                const auto greeks = engine.price_with_greeks(option, market, GreeksRequest{true});
                REQUIRE_FALSE(greeks);
                CHECK(greeks.error().category == ErrorCategory::invalid_parameter);
            }
        }
        for (const auto backend : {MonteCarloBackend::cpu, MonteCarloBackend::cuda})
            for (const auto settings : {MonteCarloSettings{1, 2, 1, backend}, {10'000'000, 10'000, 1, backend}}) {
                const auto result = MonteCarloVanillaEngine{settings}.price(
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
    const auto result = kiyosi::MonteCarloVanillaEngine{10, 2, 1}.price(call, context);
    REQUIRE(result.has_value());
    CHECK(*result == 10.0);
}

TEST_CASE("Monte Carlo engines preserve timestamp and supported date boundaries")
{
    const auto first = kiyosi::Date{std::chrono::year::min() / std::chrono::January / 1};
    const auto last = kiyosi::Date{std::chrono::year::max() / std::chrono::December / 31};
    const auto parameters = *kiyosi::make_bsm_parameters(0.0, 0.0, 0.2);
    const kiyosi::MonteCarloVanillaEngine engine{32, 3, 7};
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
        const auto baseline = kiyosi::MonteCarloVanillaEngine{20'000, 2, 42}.price(option, context);
        REQUIRE(baseline);
        CHECK_THAT(*baseline, Catch::Matchers::WithinAbs(*kiyosi::AnalyticVanillaEngine{}.price(option, context), 0.5));
        const auto different_seed = kiyosi::MonteCarloVanillaEngine{20'000, 2, 43}.price(option, context);
        REQUIRE(different_seed);
        CHECK(*different_seed != *baseline);
        for (const int step_count : {2, 7, 50, 10'000}) {
            CAPTURE(type, step_count);
            const kiyosi::MonteCarloVanillaEngine engine{19'999, step_count, 42};
            const auto price = engine.price(option, context);
            REQUIRE(price);
            CHECK(*price == *baseline);
            CHECK(engine.settings().step_count == step_count);
        }
        for (const auto backend : {kiyosi::MonteCarloBackend::cpu, kiyosi::MonteCarloBackend::cuda}) {
            for (const int step_count : {1, 10'001}) {
                const auto invalid = kiyosi::MonteCarloVanillaEngine{10, step_count, 42, backend}.price(option, context);
                REQUIRE_FALSE(invalid);
                CHECK(invalid.error().category == kiyosi::ErrorCategory::invalid_parameter);
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
    const auto result = kiyosi::MonteCarloVanillaEngine{20'000, 50, 42}.price(put, context);
    REQUIRE(result.has_value());
    CHECK_THAT(*result, Catch::Matchers::WithinAbs(50.0, 1e-10));
}

TEST_CASE("American Monte Carlo preserves sparse and singular regression fallbacks")
{
    const auto valuation = day(2025, 1, 1);
    const auto expiry_date = valuation + std::chrono::days{365};
    const auto parameters = *kiyosi::make_bsm_parameters(
        0.0, 0.10, std::numeric_limits<double>::min());
    const auto context = *kiyosi::make_pricing_context(parameters, 50.0, valuation);
    const auto put = *kiyosi::make_american_option(
        kiyosi::OptionType::put, 100.0, valuation, expiry_date);

    const auto sparse = kiyosi::MonteCarloVanillaEngine{1, 5, 42}.price(put, context);
    const auto singular = kiyosi::MonteCarloVanillaEngine{4, 5, 42}.price(put, context);

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
    const kiyosi::MonteCarloVanillaEngine implicit_cpu{20'000, 50, 42};
    const kiyosi::MonteCarloVanillaEngine explicit_cpu{
        20'000, 50, 42, kiyosi::MonteCarloBackend::cpu};

    CHECK(implicit_cpu.settings().backend == kiyosi::MonteCarloBackend::cpu);
    const auto implicit_result = implicit_cpu.price(call, context);
    const auto explicit_result = explicit_cpu.price(call, context);
    REQUIRE(implicit_result);
    REQUIRE(explicit_result);
    CHECK(*implicit_result ==
          *explicit_result);

    auto invalid_settings = implicit_cpu.settings();
    invalid_settings.backend = static_cast<kiyosi::MonteCarloBackend>(255);
    const auto invalid = kiyosi::MonteCarloVanillaEngine{invalid_settings}.price(call, context);
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
    const kiyosi::MonteCarloVanillaEngine engine{
        20'000, 50, 42, kiyosi::MonteCarloBackend::cuda};

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
    const kiyosi::MonteCarloVanillaEngine engine{
        20'000, 50, 42, kiyosi::MonteCarloBackend::cuda};

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
        const auto result = kiyosi::MonteCarloVanillaEngine{settings}.price(put, context);
        REQUIRE_FALSE(result);
        CHECK(result.error().category == kiyosi::ErrorCategory::invalid_parameter);
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
        const auto result = kiyosi::MonteCarloVanillaEngine{
            20, 3, 42, kiyosi::MonteCarloBackend::cuda}
                                .price(option, context);
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
        ~RestoreDevice() { cudaSetDevice(device); }
    } restore{original_device};
    int device_count = 0;
    REQUIRE(cudaGetDeviceCount(&device_count) == cudaSuccess);
    REQUIRE(device_count > 0);
    const auto start = day(2025, 1, 1);
    const auto end = day(2026, 1, 1);
    const auto context = *make_pricing_context(*make_bsm_parameters(0.05, 0.02, 0.2), 100.0, start);
    const auto european = *make_european_option(OptionType::call, 100.0, start, end);
    const auto american = *make_american_option(OptionType::put, 100.0, start, end);
    const auto accumulator = *make_accumulator({.strike = 100.0, .knock_out_level = 120.0,
                                                .daily_quantity = 1.0, .acceleration_factor = 2.0,
                                                .effective_date = start, .expiry_date = end});
    const auto note = *make_binary_snowball_option({.knock_out_coupon_rates = {0.1}, .maturity_coupon_rate = 0.1,
                                                    .knock_out_levels = {120.0}, .observation_dates = {end},
                                                    .effective_date = start, .expiry_date = end});
    const MonteCarloVanillaEngine vanilla{64, 4, 42, MonteCarloBackend::cuda};
    const MonteCarloAccumulatorEngine accrual{{64, 42, MonteCarloBackend::cuda}};
    const MonteCarloBinarySnowballEngine structured{{64, 42, MonteCarloBackend::cuda}};
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
        REQUIRE(structured.price(note, context));
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
    const kiyosi::MonteCarloVanillaEngine engine{
        100'000, 50, 42, kiyosi::MonteCarloBackend::cuda};

    const auto first = engine.price(call, context);
    const auto second = engine.price(call, context);
    const auto different_seed = kiyosi::MonteCarloVanillaEngine{
        100'000, 50, 43, kiyosi::MonteCarloBackend::cuda}
                                    .price(call, context);
    const auto repeated_different_seed = kiyosi::MonteCarloVanillaEngine{
        100'000, 50, 43, kiyosi::MonteCarloBackend::cuda}
                                             .price(call, context);
    const auto different_path_count = kiyosi::MonteCarloVanillaEngine{
        10'000, 50, 42, kiyosi::MonteCarloBackend::cuda}
                                          .price(call, context);
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
        const auto cpu = kiyosi::MonteCarloVanillaEngine{
            100'000, 50, 42, kiyosi::MonteCarloBackend::cpu}
                             .price(option, context);
        const auto cuda = kiyosi::MonteCarloVanillaEngine{
            100'000, 50, 42, kiyosi::MonteCarloBackend::cuda}
                              .price(option, context);
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

    const auto baseline = kiyosi::MonteCarloVanillaEngine{10'000, 2, 42, kiyosi::MonteCarloBackend::cuda}.price(call, context);
    REQUIRE(baseline);
    for (const int step_count : {2, 7, 50, 10'000}) {
        CAPTURE(step_count);
        const auto odd = kiyosi::MonteCarloVanillaEngine{
            9'999, step_count, 42, kiyosi::MonteCarloBackend::cuda}
                             .price(call, context);
        const auto even = kiyosi::MonteCarloVanillaEngine{
            10'000, step_count, 42, kiyosi::MonteCarloBackend::cuda}
                              .price(call, context);
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
    const kiyosi::MonteCarloVanillaEngine engine{
        20'000, 50, 42, kiyosi::MonteCarloBackend::cuda};
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
        const kiyosi::MonteCarloVanillaEngine engine{
            50'000, 50, 42, kiyosi::MonteCarloBackend::cuda};
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
    const auto baseline = kiyosi::MonteCarloVanillaEngine{
        50'000, 50, 42, kiyosi::MonteCarloBackend::cuda}
                              .price(put, context);
    const auto different_seed = kiyosi::MonteCarloVanillaEngine{
        50'000, 50, 43, kiyosi::MonteCarloBackend::cuda}
                                    .price(put, context);
    const auto different_path_count = kiyosi::MonteCarloVanillaEngine{
        40'000, 50, 42, kiyosi::MonteCarloBackend::cuda}
                                          .price(put, context);
    const auto cpu = kiyosi::MonteCarloVanillaEngine{
        50'000, 50, 42, kiyosi::MonteCarloBackend::cpu}
                         .price(put, context);
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
        const auto result = kiyosi::MonteCarloVanillaEngine{
            100'000, 50, 42, kiyosi::MonteCarloBackend::cuda}
                                .price(option, context);
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
        const auto cpu = kiyosi::MonteCarloVanillaEngine{
            200'000, 50, 42, kiyosi::MonteCarloBackend::cpu}
                             .price(option, context);
        const auto cuda = kiyosi::MonteCarloVanillaEngine{
            200'000, 50, 42, kiyosi::MonteCarloBackend::cuda}
                              .price(option, context);
        const auto european_cuda = kiyosi::MonteCarloVanillaEngine{
            200'000, 50, 42, kiyosi::MonteCarloBackend::cuda}
                                       .price(european_option, context);
        const auto finite_difference = kiyosi::FiniteDifferenceVanillaEngine{
            {400, 800, kiyosi::FiniteDifferenceScheme::crank_nicolson}}
                                           .price(option, context);
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
    const auto odd = kiyosi::MonteCarloVanillaEngine{
        9'999, 50, 42, kiyosi::MonteCarloBackend::cuda}
                         .price(put, context);
    const auto even = kiyosi::MonteCarloVanillaEngine{
        10'000, 50, 42, kiyosi::MonteCarloBackend::cuda}
                          .price(put, context);

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
    const auto sparse = kiyosi::MonteCarloVanillaEngine{
        1, 5, 42, kiyosi::MonteCarloBackend::cuda}
                            .price(put, context);
    const auto singular = kiyosi::MonteCarloVanillaEngine{
        4, 5, 42, kiyosi::MonteCarloBackend::cuda}
                              .price(put, context);
    const auto sparse_cpu = kiyosi::MonteCarloVanillaEngine{
        1, 5, 42, kiyosi::MonteCarloBackend::cpu}
                                .price(put, context);
    const auto singular_cpu = kiyosi::MonteCarloVanillaEngine{
        4, 5, 42, kiyosi::MonteCarloBackend::cpu}
                                  .price(put, context);

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
    const kiyosi::MonteCarloVanillaEngine engine{
        20'000, 50, 42, kiyosi::MonteCarloBackend::cuda};
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
    const auto result = kiyosi::MonteCarloVanillaEngine{
        1, 3, 42, kiyosi::MonteCarloBackend::cuda}
                            .price(call, context);

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
    const auto result = kiyosi::MonteCarloVanillaEngine{
        20, 3, 42, kiyosi::MonteCarloBackend::cuda}
                            .price(put, context);

    REQUIRE_FALSE(result);
    CHECK(result.error().category == kiyosi::ErrorCategory::invalid_result);
}
#endif

} // namespace
