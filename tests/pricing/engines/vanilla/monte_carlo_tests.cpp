#include <catch2/catch_test_macros.hpp>
#include <catch2/matchers/catch_matchers_floating_point.hpp>
#include <array>
#include <chrono>
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
    const double time = *kiyosi::year_fraction(context.valuation_time(), kiyosi::start_of_day(option.expiry()));
    const double volatility = context.parameters().volatility();
    const double dt = time / static_cast<double>(settings.step_count - 1);
    const double sqrt_dt = std::sqrt(dt);
    const double drift = (context.parameters().risk_free_rate() -
                          context.parameters().dividend_yield() -
                          0.5 * volatility * volatility) *
                         dt;
    std::mt19937_64 generator;
    generator.seed(*settings.seed);
    std::normal_distribution<double> normal;
    const int half_count = path_count / 2;
    for (int path = 0; path < half_count; ++path) {
        const auto positive = static_cast<std::size_t>(path) * stride;
        const auto negative = static_cast<std::size_t>(path + half_count) * stride;
        paths[positive] = context.asset_price();
        paths[negative] = context.asset_price();
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
    const double sign = option.type() == kiyosi::option_type::call ? 1.0 : -1.0;
    double sum = 0.0;
    for (int path = 0; path < path_count; ++path)
        sum += std::max(sign * (paths[static_cast<std::size_t>(path) * stride + stride - 1] -
                                option.strike()),
                        0.0);
    return sum / static_cast<double>(path_count) *
           std::exp(-context.parameters().risk_free_rate() * time);
}

TEST_CASE("Monte Carlo engines are deterministic, validated, and price vanilla options")
{
    const auto valuation = day(2025, 1, 1);
    const auto expiry = valuation + std::chrono::days{365};
    const auto parameters = *kiyosi::make_bsm_parameters(0.05, 0.02, 0.2);
    const auto context = *kiyosi::make_pricing_context(parameters, 100.0, valuation);
    const auto call = *kiyosi::make_european_option(kiyosi::option_type::call, 100.0, valuation, expiry);
    const auto american = *kiyosi::make_american_option(kiyosi::option_type::put, 100.0, valuation, expiry);

    const kiyosi::MonteCarloVanillaEngine european{20'000, 2, 42};
    const auto first = european.price(call, context);
    const auto second = european.price(call, context);
    REQUIRE(first.has_value());
    REQUIRE(second.has_value());
    CHECK(*first->require(kiyosi::risk_measure::price) == *second->require(kiyosi::risk_measure::price));
    CHECK(std::abs(*first->require(kiyosi::risk_measure::price) -
                   *kiyosi::AnalyticVanillaEngine{}.price(call, context)->require(kiyosi::risk_measure::price)) < 0.5);
    CHECK_FALSE(first->has(kiyosi::risk_measure::delta));

    const kiyosi::MonteCarloVanillaEngine american_engine{20'000, 20, 42};
    const auto american_result = american_engine.price(american, context);
    REQUIRE(american_result.has_value());
    CHECK(*american_result->require(kiyosi::risk_measure::price) >= 0.0);
    CHECK_FALSE(american_result->has(kiyosi::risk_measure::gamma));

    CHECK_FALSE(kiyosi::MonteCarloVanillaEngine{0, 2}.price(call, context).has_value());
    CHECK_FALSE(kiyosi::MonteCarloVanillaEngine{20, 2}.price(american, context).has_value());
}

TEST_CASE("Monte Carlo engines return intrinsic value at expiry")
{
    const auto expiry = day(2025, 1, 1);
    const auto parameters = *kiyosi::make_bsm_parameters(0.05, 0.02, 0.2);
    const auto context = *kiyosi::make_pricing_context(parameters, 110.0, expiry);
    const auto call = *kiyosi::make_european_option(kiyosi::option_type::call, 100.0, expiry, expiry);
    const auto result = kiyosi::MonteCarloVanillaEngine{10, 2, 1}.price(call, context);
    REQUIRE(result.has_value());
    CHECK(*result->require(kiyosi::risk_measure::price) == 10.0);
}

TEST_CASE("European Monte Carlo terminal retention preserves full-path seeded results")
{
    const auto valuation = day(2025, 1, 1);
    const auto expiry = valuation + std::chrono::days{365};
    const auto parameters = *kiyosi::make_bsm_parameters(0.05, 0.02, 0.2);
    const auto context = *kiyosi::make_pricing_context(parameters, 100.0, valuation);
    const auto call = *kiyosi::make_european_option(kiyosi::option_type::call, 100.0, valuation, expiry);
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
        CHECK(*result->require(kiyosi::risk_measure::price) ==
              legacy_european_price(call, context, setting));
    }
}

TEST_CASE("American Monte Carlo includes immediate exercise in the exercise window")
{
    const auto valuation = day(2025, 1, 6);
    const auto expiry = day(2026, 1, 6);
    const auto parameters = *kiyosi::make_bsm_parameters(0.10, 0.0, 0.10);
    const auto context = *kiyosi::make_pricing_context(parameters, 50.0, valuation);
    const auto put = *kiyosi::make_american_option(kiyosi::option_type::put, 100.0, valuation, expiry);
    const auto result = kiyosi::MonteCarloVanillaEngine{20'000, 50, 42}.price(put, context);
    REQUIRE(result.has_value());
    CHECK_THAT(*result->require(kiyosi::risk_measure::price), Catch::Matchers::WithinAbs(50.0, 1e-10));
}

TEST_CASE("American Monte Carlo preserves sparse and singular regression fallbacks")
{
    const auto valuation = day(2025, 1, 1);
    const auto expiry = valuation + std::chrono::days{365};
    const auto parameters = *kiyosi::make_bsm_parameters(
        0.0, 0.10, std::numeric_limits<double>::min());
    const auto context = *kiyosi::make_pricing_context(parameters, 50.0, valuation);
    const auto put = *kiyosi::make_american_option(
        kiyosi::option_type::put, 100.0, valuation, expiry);

    const auto sparse = kiyosi::MonteCarloVanillaEngine{1, 5, 42}.price(put, context);
    const auto singular = kiyosi::MonteCarloVanillaEngine{4, 5, 42}.price(put, context);

    REQUIRE(sparse);
    REQUIRE(singular);
    CHECK(*sparse->require(kiyosi::risk_measure::price) ==
          *singular->require(kiyosi::risk_measure::price));
    CHECK(*sparse->require(kiyosi::risk_measure::price) > 50.0);
}

TEST_CASE("Monte Carlo defaults to CPU and preserves explicit CPU pricing")
{
    const auto valuation = day(2025, 1, 1);
    const auto expiry = valuation + std::chrono::days{365};
    const auto parameters = *kiyosi::make_bsm_parameters(0.05, 0.02, 0.2);
    const auto context = *kiyosi::make_pricing_context(parameters, 100.0, valuation);
    const auto call = *kiyosi::make_european_option(
        kiyosi::option_type::call, 100.0, valuation, expiry);
    const kiyosi::MonteCarloVanillaEngine implicit_cpu{20'000, 50, 42};
    const kiyosi::MonteCarloVanillaEngine explicit_cpu{
        20'000, 50, 42, kiyosi::monte_carlo_backend::cpu};

    CHECK(implicit_cpu.settings().backend == kiyosi::monte_carlo_backend::cpu);
    const auto implicit_result = implicit_cpu.price(call, context);
    const auto explicit_result = explicit_cpu.price(call, context);
    REQUIRE(implicit_result);
    REQUIRE(explicit_result);
    CHECK(*implicit_result->require(kiyosi::risk_measure::price) ==
          *explicit_result->require(kiyosi::risk_measure::price));

    auto invalid_settings = implicit_cpu.settings();
    invalid_settings.backend = static_cast<kiyosi::monte_carlo_backend>(255);
    const auto invalid = kiyosi::MonteCarloVanillaEngine{invalid_settings}.price(call, context);
    REQUIRE_FALSE(invalid);
    CHECK(invalid.error().category == kiyosi::error_category::invalid_parameter);
}

#if !KIYOSI_HAS_CUDA
TEST_CASE("CUDA selection is deferred and unavailable builds do not fall back")
{
    const auto valuation = day(2025, 1, 1);
    const auto expiry = valuation + std::chrono::days{365};
    const auto parameters = *kiyosi::make_bsm_parameters(0.05, 0.02, 0.2);
    const auto context = *kiyosi::make_pricing_context(parameters, 100.0, valuation);
    const auto call = *kiyosi::make_european_option(
        kiyosi::option_type::call, 100.0, valuation, expiry);
    const kiyosi::MonteCarloVanillaEngine engine{
        20'000, 50, 42, kiyosi::monte_carlo_backend::cuda};

    CHECK(engine.settings().backend == kiyosi::monte_carlo_backend::cuda);
    const auto result = engine.price(call, context);
    REQUIRE_FALSE(result);
    CHECK(result.error().category == kiyosi::error_category::backend_unavailable);
}
#endif

TEST_CASE("CUDA rejects American Monte Carlo without CPU fallback")
{
    const auto valuation = day(2025, 1, 1);
    const auto expiry = valuation + std::chrono::days{365};
    const auto parameters = *kiyosi::make_bsm_parameters(0.05, 0.02, 0.2);
    const auto context = *kiyosi::make_pricing_context(parameters, 100.0, valuation);
    const auto put = *kiyosi::make_american_option(
        kiyosi::option_type::put, 100.0, valuation, expiry);
    const kiyosi::MonteCarloVanillaEngine engine{
        20'000, 50, 42, kiyosi::monte_carlo_backend::cuda};

    const auto result = engine.price(put, context);
    REQUIRE_FALSE(result);
    CHECK(result.error().category == kiyosi::error_category::unsupported_operation);
}

#if KIYOSI_HAS_CUDA
TEST_CASE("CUDA European Monte Carlo is seeded and deterministic", "[cuda]")
{
    const auto valuation = day(2025, 1, 1);
    const auto expiry = valuation + std::chrono::days{365};
    const auto parameters = *kiyosi::make_bsm_parameters(0.05, 0.02, 0.2);
    const auto context = *kiyosi::make_pricing_context(parameters, 100.0, valuation);
    const auto call = *kiyosi::make_european_option(
        kiyosi::option_type::call, 100.0, valuation, expiry);
    const kiyosi::MonteCarloVanillaEngine engine{
        100'000, 50, 42, kiyosi::monte_carlo_backend::cuda};

    const auto first = engine.price(call, context);
    const auto second = engine.price(call, context);
    const auto different_seed = kiyosi::MonteCarloVanillaEngine{
        100'000, 50, 43, kiyosi::monte_carlo_backend::cuda}.price(call, context);
    const auto repeated_different_seed = kiyosi::MonteCarloVanillaEngine{
        100'000, 50, 43, kiyosi::monte_carlo_backend::cuda}.price(call, context);
    const auto different_path_count = kiyosi::MonteCarloVanillaEngine{
        10'000, 50, 42, kiyosi::monte_carlo_backend::cuda}.price(call, context);
    REQUIRE(first);
    REQUIRE(second);
    REQUIRE(different_seed);
    REQUIRE(repeated_different_seed);
    REQUIRE(different_path_count);
    CHECK(*first->require(kiyosi::risk_measure::price) ==
          *second->require(kiyosi::risk_measure::price));
    CHECK(*different_seed->require(kiyosi::risk_measure::price) ==
          *repeated_different_seed->require(kiyosi::risk_measure::price));
    CHECK(*first->require(kiyosi::risk_measure::price) !=
          *different_seed->require(kiyosi::risk_measure::price));
    CHECK(*first->require(kiyosi::risk_measure::price) !=
          *different_path_count->require(kiyosi::risk_measure::price));
    CHECK(*first->require(kiyosi::risk_measure::price) >= 0.0);
    CHECK_FALSE(first->has(kiyosi::risk_measure::delta));
}

TEST_CASE("CUDA European Monte Carlo agrees with CPU and analytic prices", "[cuda]")
{
    struct Case {
        kiyosi::option_type type;
        double strike;
    };
    constexpr std::array cases{
        Case{kiyosi::option_type::call, 90.0},
        Case{kiyosi::option_type::call, 100.0},
        Case{kiyosi::option_type::call, 110.0},
        Case{kiyosi::option_type::put, 100.0},
    };
    constexpr double tolerance = 0.35;
    const auto valuation = day(2025, 1, 1);
    const auto expiry = valuation + std::chrono::days{365};
    const auto parameters = *kiyosi::make_bsm_parameters(0.05, 0.02, 0.2);
    const auto context = *kiyosi::make_pricing_context(parameters, 100.0, valuation);

    for (const auto test : cases) {
        CAPTURE(test.type, test.strike);
        const auto option = *kiyosi::make_european_option(
            test.type, test.strike, valuation, expiry);
        const auto cpu = kiyosi::MonteCarloVanillaEngine{
            100'000, 50, 42, kiyosi::monte_carlo_backend::cpu}.price(option, context);
        const auto cuda = kiyosi::MonteCarloVanillaEngine{
            100'000, 50, 42, kiyosi::monte_carlo_backend::cuda}.price(option, context);
        const auto analytic = kiyosi::AnalyticVanillaEngine{}.price(option, context);
        REQUIRE(cpu);
        REQUIRE(cuda);
        REQUIRE(analytic);
        const double cuda_price = *cuda->require(kiyosi::risk_measure::price);
        CHECK_THAT(cuda_price, Catch::Matchers::WithinAbs(
                                   *cpu->require(kiyosi::risk_measure::price), tolerance));
        CHECK_THAT(cuda_price, Catch::Matchers::WithinAbs(
                                   *analytic->require(kiyosi::risk_measure::price), tolerance));
    }
}

TEST_CASE("CUDA European Monte Carlo rounds odd path counts for antithetic pairs", "[cuda]")
{
    const auto valuation = day(2025, 1, 1);
    const auto expiry = valuation + std::chrono::days{365};
    const auto parameters = *kiyosi::make_bsm_parameters(0.05, 0.02, 0.2);
    const auto context = *kiyosi::make_pricing_context(parameters, 100.0, valuation);
    const auto call = *kiyosi::make_european_option(
        kiyosi::option_type::call, 100.0, valuation, expiry);

    for (const int step_count : {2, 7, 50}) {
        CAPTURE(step_count);
        const auto odd = kiyosi::MonteCarloVanillaEngine{
            9'999, step_count, 42, kiyosi::monte_carlo_backend::cuda}.price(call, context);
        const auto even = kiyosi::MonteCarloVanillaEngine{
            10'000, step_count, 42, kiyosi::monte_carlo_backend::cuda}.price(call, context);
        REQUIRE(odd);
        REQUIRE(even);
        CHECK(*odd->require(kiyosi::risk_measure::price) ==
              *even->require(kiyosi::risk_measure::price));
    }
}

TEST_CASE("CUDA Monte Carlo supports concurrent const pricing", "[cuda]")
{
    const auto valuation = day(2025, 1, 1);
    const auto expiry = valuation + std::chrono::days{365};
    const auto parameters = *kiyosi::make_bsm_parameters(0.05, 0.02, 0.2);
    const auto context = *kiyosi::make_pricing_context(parameters, 100.0, valuation);
    const auto call = *kiyosi::make_european_option(
        kiyosi::option_type::call, 100.0, valuation, expiry);
    const kiyosi::MonteCarloVanillaEngine engine{
        20'000, 50, 42, kiyosi::monte_carlo_backend::cuda};
    const auto baseline = engine.price(call, context);
    REQUIRE(baseline);

    std::array<std::future<kiyosi::result<kiyosi::PricingResult>>, 4> results;
    for (auto& result : results)
        result = std::async(std::launch::async, [&] { return engine.price(call, context); });
    for (auto& pending : results) {
        const auto result = pending.get();
        REQUIRE(result);
        CHECK(*result->require(kiyosi::risk_measure::price) ==
              *baseline->require(kiyosi::risk_measure::price));
    }
}
#endif

} // namespace
