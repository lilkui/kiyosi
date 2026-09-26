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

TEST_CASE("Finite-difference European engines track analytic prices")
{
    using Catch::Matchers::WithinAbs;
    const auto valuation = day(2025, 1, 1);
    const auto expiry_date = valuation + std::chrono::days{365};
    const auto parameters = *kiyosi::make_bsm_parameters(0.05, 0.0, 0.2);
    const auto context = *kiyosi::make_pricing_context(parameters, 100.0, valuation);
    const auto call = *kiyosi::make_european_option(kiyosi::OptionType::call, 100.0, valuation, expiry_date);
    const auto analytic = *kiyosi::AnalyticVanillaEngine{}.price(call, context);
    const kiyosi::FiniteDifferenceSettings settings{200, 400, kiyosi::FiniteDifferenceScheme::crank_nicolson};
    const auto european = kiyosi::FiniteDifferenceVanillaEngine{settings}.price(call, context);
    REQUIRE(european.has_value());
    CHECK_THAT(*european,
               WithinAbs(analytic, 0.05));
    for (const auto scheme : {kiyosi::FiniteDifferenceScheme::explicit_euler,
                              kiyosi::FiniteDifferenceScheme::implicit_euler}) {
        const auto result = kiyosi::FiniteDifferenceVanillaEngine{{200, 2000, scheme}}.price(call, context);
        REQUIRE(result.has_value());
        CHECK_THAT(*result,
                   WithinAbs(analytic, 0.15));
    }
}

TEST_CASE("Finite-difference American engines track analytic and binomial prices")
{
    using Catch::Matchers::WithinAbs;
    const auto valuation = day(2025, 1, 1);
    const auto expiry_date = valuation + std::chrono::days{365};
    const auto parameters = *kiyosi::make_bsm_parameters(0.05, 0.0, 0.2);
    const auto context = *kiyosi::make_pricing_context(parameters, 100.0, valuation);
    const auto call = *kiyosi::make_european_option(kiyosi::OptionType::call, 100.0, valuation, expiry_date);
    const auto american_call = *kiyosi::make_american_option(
        kiyosi::OptionType::call, 100.0, valuation, expiry_date);
    const auto analytic = *kiyosi::AnalyticVanillaEngine{}.price(call, context);
    const kiyosi::FiniteDifferenceSettings settings{200, 400, kiyosi::FiniteDifferenceScheme::crank_nicolson};
    const auto american = kiyosi::FiniteDifferenceVanillaEngine{settings}.price(american_call, context);
    REQUIRE(american.has_value());
    CHECK_THAT(*american,
               WithinAbs(analytic, 0.05));
    const auto put = *kiyosi::make_american_option(kiyosi::OptionType::put, 100.0, valuation, expiry_date);
    const auto finite_put = kiyosi::FiniteDifferenceVanillaEngine{settings}.price(put, context);
    const auto tree_put = kiyosi::CoxRossRubinsteinVanillaEngine{kiyosi::BinomialSettings{400}}.price(put, context);
    REQUIRE(finite_put.has_value());
    REQUIRE(tree_put.has_value());
    CHECK_THAT(*finite_put,
               WithinAbs(*tree_put, 0.1));
}

TEST_CASE("Finite-difference engines reject invalid grids")
{
    const auto valuation = day(2025, 1, 1);
    const auto expiry_date = valuation + std::chrono::days{365};
    const auto parameters = *kiyosi::make_bsm_parameters(0.05, 0.0, 0.2);
    const auto context = *kiyosi::make_pricing_context(parameters, 100.0, valuation);
    const auto call = *kiyosi::make_european_option(kiyosi::OptionType::call, 100.0, valuation, expiry_date);
    const kiyosi::FiniteDifferenceVanillaEngine invalid_engine{
        {0, 10, kiyosi::FiniteDifferenceScheme::implicit_euler}};
    CHECK(invalid_engine.settings().asset_step_count == 0);
    const auto invalid_grid = invalid_engine.price(call, context);
    REQUIRE_FALSE(invalid_grid);
    CHECK(invalid_grid.error().category == kiyosi::ErrorCategory::invalid_parameter);
    CHECK_FALSE(kiyosi::FiniteDifferenceVanillaEngine{{200, 200,
                                                       kiyosi::FiniteDifferenceScheme::crank_nicolson, -1.0}}
                    .price(call, context)
                    .has_value());
    CHECK_FALSE(kiyosi::FiniteDifferenceSettings{}.asset_upper_boundary.has_value());
}

TEST_CASE("Finite-difference American engine returns intrinsic value at expiry_date")
{
    const auto expiry_date = day(2026, 1, 1);
    const auto parameters = *kiyosi::make_bsm_parameters(0.05, 0.0, 0.2);
    const auto expiry_context = *kiyosi::make_pricing_context(parameters, 90.0, expiry_date);
    const auto expiry_put = *kiyosi::make_american_option(kiyosi::OptionType::put, 100.0, expiry_date, expiry_date);
    const auto at_expiry = kiyosi::FiniteDifferenceVanillaEngine{}.price(expiry_put, expiry_context);
    REQUIRE(at_expiry.has_value());
    CHECK(*at_expiry == 10.0);
}

} // namespace
