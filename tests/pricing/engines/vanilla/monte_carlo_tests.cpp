#include <catch2/catch_test_macros.hpp>
#include <catch2/matchers/catch_matchers_floating_point.hpp>
#include <array>
#include <chrono>
#include <limits>
#include <string>
#include <type_traits>
#include <vector>
#include <kiyosi/kiyosi.hpp>
#include "support/common.hpp"

namespace {

using kiyosi::test::day;

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

} // namespace
