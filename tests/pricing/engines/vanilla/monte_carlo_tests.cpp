#include <catch2/catch_test_macros.hpp>
#include <catch2/matchers/catch_matchers_floating_point.hpp>
#include <array>
#include <chrono>
#include <cmath>
#include <future>
#include <limits>
#include <random>
#include <string>
#include <type_traits>
#include <vector>
#include <kiyosi/kiyosi.hpp>
#include "support/common.hpp"

namespace {

using kiyosi::test::day;

double legacy_european_price(const kiyosi::EuropeanOption& option,
                             const kiyosi::PricingContext& context,
                             kiyosi::MonteCarloSettings settings)
{
    const int path_count = settings.path_count % 2 == 0 ? settings.path_count : settings.path_count + 1;
    const auto stride = static_cast<std::size_t>(settings.step_count);
    std::vector<double> paths(static_cast<std::size_t>(path_count) * stride);
    const double time = *kiyosi::year_fraction(context.valuation_time(), kiyosi::start_of_day(option.expiry_date()));
    const double volatility = context.model_parameters().volatility();
    const double dt = time / static_cast<double>(settings.step_count - 1);
    const double sqrt_dt = std::sqrt(dt);
    const double drift = (context.model_parameters().risk_free_rate() -
                          context.model_parameters().dividend_yield() -
                          0.5 * volatility * volatility) *
                         dt;
    std::mt19937_64 generator;
    generator.seed(*settings.seed);
    std::normal_distribution<double> normal;
    const int half_count = path_count / 2;
    for (int path = 0; path < half_count; ++path) {
        const auto positive = static_cast<std::size_t>(path) * stride;
        const auto negative = static_cast<std::size_t>(path + half_count) * stride;
        paths[positive] = context.spot_price();
        paths[negative] = context.spot_price();
        for (int step = 1; step < settings.step_count; ++step) {
            const double normal_draw = normal(generator);
            paths[positive + static_cast<std::size_t>(step)] =
                paths[positive + static_cast<std::size_t>(step - 1)] *
                std::exp(drift + volatility * sqrt_dt * normal_draw);
            paths[negative + static_cast<std::size_t>(step)] =
                paths[negative + static_cast<std::size_t>(step - 1)] *
                std::exp(drift - volatility * sqrt_dt * normal_draw);
        }
    }
    const double sign = option.option_type() == kiyosi::OptionType::call ? 1.0 : -1.0;
    double sum = 0.0;
    for (int path = 0; path < path_count; ++path)
        sum += std::max(sign * (paths[static_cast<std::size_t>(path) * stride + stride - 1] -
                                option.strike()),
                        0.0);
    return sum / static_cast<double>(path_count) *
           std::exp(-context.model_parameters().risk_free_rate() * time);
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
    CHECK(*first->require(kiyosi::RiskMeasure::price) == *second->require(kiyosi::RiskMeasure::price));
    CHECK(std::abs(*first->require(kiyosi::RiskMeasure::price) -
                   *kiyosi::AnalyticVanillaEngine{}.price(call, context)->require(kiyosi::RiskMeasure::price)) < 0.5);
    CHECK_FALSE(first->has(kiyosi::RiskMeasure::delta));

    const kiyosi::MonteCarloVanillaEngine american_engine{20'000, 20, 42};
    const auto american_result = american_engine.price(american, context);
    REQUIRE(american_result.has_value());
    CHECK(*american_result->require(kiyosi::RiskMeasure::price) >= 0.0);
    CHECK_FALSE(american_result->has(kiyosi::RiskMeasure::gamma));

    CHECK_FALSE(kiyosi::MonteCarloVanillaEngine{0, 2}.price(call, context).has_value());
    CHECK_FALSE(kiyosi::MonteCarloVanillaEngine{20, 2}.price(american, context).has_value());
}

TEST_CASE("Monte Carlo engines return intrinsic value at expiry_date")
{
    const auto expiry_date = day(2025, 1, 1);
    const auto parameters = *kiyosi::make_bsm_parameters(0.05, 0.02, 0.2);
    const auto context = *kiyosi::make_pricing_context(parameters, 110.0, expiry_date);
    const auto call = *kiyosi::make_european_option(kiyosi::OptionType::call, 100.0, expiry_date, expiry_date);
    const auto result = kiyosi::MonteCarloVanillaEngine{10, 2, 1}.price(call, context);
    REQUIRE(result.has_value());
    CHECK(*result->require(kiyosi::RiskMeasure::price) == 10.0);
}

TEST_CASE("European Monte Carlo terminal retention preserves full-path seeded results")
{
    const auto valuation = day(2025, 1, 1);
    const auto expiry_date = valuation + std::chrono::days{365};
    const auto parameters = *kiyosi::make_bsm_parameters(0.05, 0.02, 0.2);
    const auto context = *kiyosi::make_pricing_context(parameters, 100.0, valuation);
    const auto call = *kiyosi::make_european_option(kiyosi::OptionType::call, 100.0, valuation, expiry_date);
    const std::array settings{
        kiyosi::MonteCarloSettings{10, 2, 42},
        kiyosi::MonteCarloSettings{9, 2, 42},
        kiyosi::MonteCarloSettings{10, 7, 42},
        kiyosi::MonteCarloSettings{9, 7, 42},
    };

    for (const auto setting : settings) {
        CAPTURE(setting.path_count, setting.step_count);
        const auto result = kiyosi::MonteCarloVanillaEngine{setting}.price(call, context);
        REQUIRE(result.has_value());
        CHECK(*result->require(kiyosi::RiskMeasure::price) ==
              legacy_european_price(call, context, setting));
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
    CHECK_THAT(*result->require(kiyosi::RiskMeasure::price), Catch::Matchers::WithinAbs(50.0, 1e-10));
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
    CHECK(*sparse->require(kiyosi::RiskMeasure::price) ==
          *singular->require(kiyosi::RiskMeasure::price));
    CHECK(*sparse->require(kiyosi::RiskMeasure::price) > 50.0);
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
    CHECK(*implicit_result->require(kiyosi::RiskMeasure::price) ==
          *explicit_result->require(kiyosi::RiskMeasure::price));

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
        CHECK(*result->require(kiyosi::RiskMeasure::price) == test.expected);
    }
}

#if KIYOSI_HAS_CUDA
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
    CHECK(*first->require(kiyosi::RiskMeasure::price) ==
          *second->require(kiyosi::RiskMeasure::price));
    CHECK(*different_seed->require(kiyosi::RiskMeasure::price) ==
          *repeated_different_seed->require(kiyosi::RiskMeasure::price));
    CHECK(*first->require(kiyosi::RiskMeasure::price) !=
          *different_seed->require(kiyosi::RiskMeasure::price));
    CHECK(*first->require(kiyosi::RiskMeasure::price) !=
          *different_path_count->require(kiyosi::RiskMeasure::price));
    CHECK(*first->require(kiyosi::RiskMeasure::price) >= 0.0);
    CHECK_FALSE(first->has(kiyosi::RiskMeasure::delta));
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
        const double cuda_price = *cuda->require(kiyosi::RiskMeasure::price);
        CHECK_THAT(cuda_price, Catch::Matchers::WithinAbs(
                                   *cpu->require(kiyosi::RiskMeasure::price), tolerance));
        CHECK_THAT(cuda_price, Catch::Matchers::WithinAbs(
                                   *analytic->require(kiyosi::RiskMeasure::price), tolerance));
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

    for (const int step_count : {2, 7, 50}) {
        CAPTURE(step_count);
        const auto odd = kiyosi::MonteCarloVanillaEngine{
            9'999, step_count, 42, kiyosi::MonteCarloBackend::cuda}
                             .price(call, context);
        const auto even = kiyosi::MonteCarloVanillaEngine{
            10'000, step_count, 42, kiyosi::MonteCarloBackend::cuda}
                              .price(call, context);
        REQUIRE(odd);
        REQUIRE(even);
        CHECK(*odd->require(kiyosi::RiskMeasure::price) ==
              *even->require(kiyosi::RiskMeasure::price));
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

    std::array<std::future<kiyosi::Result<kiyosi::PricingResult>>, 4> results;
    for (auto& result : results)
        result = std::async(std::launch::async, [&] { return engine.price(call, context); });
    for (auto& pending : results) {
        const auto result = pending.get();
        REQUIRE(result);
        CHECK(*result->require(kiyosi::RiskMeasure::price) ==
              *baseline->require(kiyosi::RiskMeasure::price));
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
        CHECK(*first->require(kiyosi::RiskMeasure::price) ==
              *second->require(kiyosi::RiskMeasure::price));
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
    CHECK(*baseline->require(kiyosi::RiskMeasure::price) !=
          *different_seed->require(kiyosi::RiskMeasure::price));
    CHECK(*baseline->require(kiyosi::RiskMeasure::price) !=
          *different_path_count->require(kiyosi::RiskMeasure::price));
    CHECK(*baseline->require(kiyosi::RiskMeasure::price) !=
          *cpu->require(kiyosi::RiskMeasure::price));
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
        CHECK_THAT(*result->require(kiyosi::RiskMeasure::price),
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
        const double cuda_price = *cuda->require(kiyosi::RiskMeasure::price);
        CHECK(cuda_price > *european_cuda->require(kiyosi::RiskMeasure::price));
        CHECK_THAT(cuda_price, Catch::Matchers::WithinAbs(
                                   *cpu->require(kiyosi::RiskMeasure::price), tolerance));
        CHECK_THAT(cuda_price, Catch::Matchers::WithinAbs(
                                   *finite_difference->require(kiyosi::RiskMeasure::price),
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
    CHECK(*odd->require(kiyosi::RiskMeasure::price) ==
          *even->require(kiyosi::RiskMeasure::price));
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
    CHECK(*sparse->require(kiyosi::RiskMeasure::price) ==
          *singular->require(kiyosi::RiskMeasure::price));
    CHECK_THAT(*sparse->require(kiyosi::RiskMeasure::price),
               Catch::Matchers::WithinAbs(
                   *sparse_cpu->require(kiyosi::RiskMeasure::price), 1e-10));
    CHECK_THAT(*singular->require(kiyosi::RiskMeasure::price),
               Catch::Matchers::WithinAbs(
                   *singular_cpu->require(kiyosi::RiskMeasure::price), 1e-10));
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

    std::array<std::future<kiyosi::Result<kiyosi::PricingResult>>, 4> results;
    for (auto& result : results)
        result = std::async(std::launch::async, [&] { return engine.price(put, context); });
    for (auto& pending : results) {
        const auto result = pending.get();
        REQUIRE(result);
        CHECK(*result->require(kiyosi::RiskMeasure::price) ==
              *baseline->require(kiyosi::RiskMeasure::price));
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
    CHECK_THAT(*result->require(kiyosi::RiskMeasure::price),
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
