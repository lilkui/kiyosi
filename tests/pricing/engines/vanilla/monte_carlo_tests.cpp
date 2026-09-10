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
    const auto context = *kiyosi::make_pricing_context(parameters, *kiyosi::make_asset_price(100.0), valuation);
    const auto call = *kiyosi::make_european_call(100.0, expiry);
    const auto american = *kiyosi::make_american_put(100.0, expiry);

    const kiyosi::MonteCarloEuropeanEngine european{20'000, 2, 42};
    const auto first = european.price(call, context);
    const auto second = european.price(call, context);
    REQUIRE(first.has_value());
    REQUIRE(second.has_value());
    CHECK(*first->get(kiyosi::risk_measure::price) == *second->get(kiyosi::risk_measure::price));
    CHECK(std::abs(*first->get(kiyosi::risk_measure::price) -
                   *kiyosi::AnalyticEuropeanEngine{}.price(call, context)->get(kiyosi::risk_measure::price)) < 0.5);
    CHECK_FALSE(first->has(kiyosi::risk_measure::delta));

    const kiyosi::MonteCarloAmericanEngine american_engine{20'000, 20, 42};
    const auto american_result = american_engine.price(american, context);
    REQUIRE(american_result.has_value());
    CHECK(*american_result->get(kiyosi::risk_measure::price) >= 0.0);
    CHECK_FALSE(american_result->has(kiyosi::risk_measure::gamma));

    CHECK_FALSE(kiyosi::MonteCarloEuropeanEngine{0, 2}.price(call, context).has_value());
    CHECK_FALSE(kiyosi::MonteCarloAmericanEngine{20, 2}.price(american, context).has_value());
}

TEST_CASE("Monte Carlo engines return intrinsic value at expiry")
{
    const auto expiry = day(2025, 1, 1);
    const auto parameters = *kiyosi::make_bsm_parameters(0.05, 0.02, 0.2);
    const auto context = *kiyosi::make_pricing_context(parameters, *kiyosi::make_asset_price(110.0), expiry);
    const auto call = *kiyosi::make_european_call(100.0, expiry);
    const auto result = kiyosi::MonteCarloEuropeanEngine{10, 2, 1}.price(call, context);
    REQUIRE(result.has_value());
    CHECK(*result->get(kiyosi::risk_measure::price) == 10.0);
}

}
