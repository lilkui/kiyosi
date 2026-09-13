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
using kiyosi::test::risk_value;

TEST_CASE("Finite-difference European engines track analytic prices")
{
    using Catch::Matchers::WithinAbs;
    const auto valuation = day(2025, 1, 1);
    const auto expiry = valuation + std::chrono::days{365};
    const auto parameters = *kiyosi::make_bsm_parameters(0.05, 0.0, 0.2);
    const auto context = *kiyosi::make_pricing_context(parameters, 100.0, valuation);
    const auto call = *kiyosi::make_european_option(kiyosi::option_type::call, 100.0, valuation, expiry);
    const auto american_call = *kiyosi::make_american_option(
        kiyosi::option_type::call, 100.0, valuation, expiry);
    const auto analytic = *kiyosi::AnalyticEuropeanEngine{}.price(call, context);
    const kiyosi::FiniteDifferenceSettings settings{200, 400, kiyosi::finite_difference_scheme::crank_nicolson};
    const auto european = kiyosi::FiniteDifferenceEuropeanEngine{settings}.price(call, context);
    REQUIRE(european.has_value());
    CHECK_THAT(risk_value(*european, kiyosi::risk_measure::price),
               WithinAbs(risk_value(analytic, kiyosi::risk_measure::price), 0.05));
    for (const auto scheme : {kiyosi::finite_difference_scheme::explicit_euler,
                              kiyosi::finite_difference_scheme::implicit_euler}) {
        const auto result = kiyosi::FiniteDifferenceEuropeanEngine{{200, 2000, scheme}}.price(call, context);
        REQUIRE(result.has_value());
        CHECK_THAT(risk_value(*result, kiyosi::risk_measure::price),
                   WithinAbs(risk_value(analytic, kiyosi::risk_measure::price), 0.15));
    }
}

TEST_CASE("Finite-difference American engines track analytic and binomial prices")
{
    using Catch::Matchers::WithinAbs;
    const auto valuation = day(2025, 1, 1);
    const auto expiry = valuation + std::chrono::days{365};
    const auto parameters = *kiyosi::make_bsm_parameters(0.05, 0.0, 0.2);
    const auto context = *kiyosi::make_pricing_context(parameters, 100.0, valuation);
    const auto call = *kiyosi::make_european_option(kiyosi::option_type::call, 100.0, valuation, expiry);
    const auto american_call = *kiyosi::make_american_option(
        kiyosi::option_type::call, 100.0, valuation, expiry);
    const auto analytic = *kiyosi::AnalyticEuropeanEngine{}.price(call, context);
    const kiyosi::FiniteDifferenceSettings settings{200, 400, kiyosi::finite_difference_scheme::crank_nicolson};
    const auto american = kiyosi::FiniteDifferenceAmericanEngine{settings}.price(american_call, context);
    REQUIRE(american.has_value());
    CHECK_THAT(risk_value(*american, kiyosi::risk_measure::price),
               WithinAbs(risk_value(analytic, kiyosi::risk_measure::price), 0.05));
    const auto put = *kiyosi::make_american_option(kiyosi::option_type::put, 100.0, valuation, expiry);
    const auto finite_put = kiyosi::FiniteDifferenceAmericanEngine{settings}.price(put, context);
    const auto tree_put = kiyosi::CrrVanillaEngine{kiyosi::BinomialSettings{400}}.price(put, context);
    REQUIRE(finite_put.has_value());
    REQUIRE(tree_put.has_value());
    CHECK_THAT(risk_value(*finite_put, kiyosi::risk_measure::price),
               WithinAbs(risk_value(*tree_put, kiyosi::risk_measure::price), 0.1));
}

TEST_CASE("Finite-difference engines reject invalid grids")
{
    const auto valuation = day(2025, 1, 1);
    const auto expiry = valuation + std::chrono::days{365};
    const auto parameters = *kiyosi::make_bsm_parameters(0.05, 0.0, 0.2);
    const auto context = *kiyosi::make_pricing_context(parameters, 100.0, valuation);
    const auto call = *kiyosi::make_european_option(kiyosi::option_type::call, 100.0, valuation, expiry);
    CHECK_FALSE(kiyosi::FiniteDifferenceEuropeanEngine{{2, 10, kiyosi::finite_difference_scheme::implicit_euler}}
                    .price(call, context)
                    .has_value());
}

TEST_CASE("Finite-difference American engine returns intrinsic value at expiry")
{
    const auto expiry = day(2026, 1, 1);
    const auto parameters = *kiyosi::make_bsm_parameters(0.05, 0.0, 0.2);
    const auto expiry_context = *kiyosi::make_pricing_context(parameters, 90.0, expiry);
    const auto expiry_put = *kiyosi::make_american_option(kiyosi::option_type::put, 100.0, expiry, expiry);
    const auto at_expiry = kiyosi::FiniteDifferenceAmericanEngine{}.price(expiry_put, expiry_context);
    REQUIRE(at_expiry.has_value());
    CHECK(risk_value(*at_expiry, kiyosi::risk_measure::price) == 10.0);
}

} // namespace
