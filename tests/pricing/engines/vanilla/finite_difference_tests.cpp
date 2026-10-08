#include <catch2/catch_test_macros.hpp>
#include <catch2/matchers/catch_matchers_floating_point.hpp>
#include <array>
#include <chrono>
#include <cmath>
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

TEST_CASE("Finite-difference boundaries preserve long-expiry volatility tails")
{
    const auto start = day(2025, 1, 1);
    const auto end = day(2035, 1, 1);
    const auto context = *kiyosi::make_pricing_context(*kiyosi::make_bsm_parameters(0.03, 0.02, 0.6), 100.0, start);
    for (const auto type : {kiyosi::OptionType::call, kiyosi::OptionType::put}) {
        CAPTURE(type);
        const auto option = *kiyosi::make_european_option(type, 100.0, start, end);
        const auto expected = kiyosi::AnalyticVanillaEngine{}.price(option, context);
        REQUIRE(expected);
        for (const int steps : {200, 800, 1600}) {
            CAPTURE(steps);
            for (const std::optional<double> upper : {std::optional<double>{}, std::optional<double>{125.0}}) {
                CAPTURE(upper.has_value());
                // A narrow domain needs finer time steps to damp the terminal strike kink.
                const int time_steps = upper ? 2000 : steps;
                const auto actual = kiyosi::FiniteDifferenceVanillaEngine{{steps, time_steps, kiyosi::FiniteDifferenceScheme::crank_nicolson, upper}}.price(option, context);
                REQUIRE(actual);
                CHECK_THAT(*actual, Catch::Matchers::WithinAbs(*expected, 0.01));
            }
        }
        const auto cash = *kiyosi::make_cash_or_nothing_option(type, 100.0, 10.0, start, end);
        const auto asset = *kiyosi::make_asset_or_nothing_option(type, 100.0, start, end);
        const kiyosi::FiniteDifferenceDigitalEngine digital{800, 1000};
        const auto cash_price = digital.price(cash, context);
        const auto asset_price = digital.price(asset, context);
        REQUIRE(cash_price);
        REQUIRE(asset_price);
        CHECK_THAT(*cash_price, Catch::Matchers::WithinAbs(*kiyosi::AnalyticDigitalEngine{}.price(cash, context), 0.001));
        CHECK_THAT(*asset_price, Catch::Matchers::WithinAbs(*kiyosi::AnalyticDigitalEngine{}.price(asset, context), 0.01));
    }
}

TEST_CASE("American finite-difference boundaries preserve continuation value", "[audit-fixes]")
{
    const auto valuation = day(2025, 1, 1);
    const auto expiry = day(2026, 1, 1);
    const kiyosi::FiniteDifferenceVanillaEngine engine{{800, 800, kiyosi::FiniteDifferenceScheme::crank_nicolson, 400.0}};
    for (const bool call : {true, false}) {
        CAPTURE(call);
        const auto type = call ? kiyosi::OptionType::call : kiyosi::OptionType::put;
        const double spot = call ? 390.0 : 0.1;
        const auto parameters = *kiyosi::make_bsm_parameters(call ? 0.05 : -0.05, 0.0, 0.2);
        const auto context = *kiyosi::make_pricing_context(parameters, spot, valuation);
        const auto american = *kiyosi::make_american_option(type, 100.0, valuation, expiry);
        const auto european = *kiyosi::make_european_option(type, 100.0, valuation, expiry);
        const auto price = engine.price(american, context);
        const auto continuation = kiyosi::AnalyticVanillaEngine{}.price(european, context);
        REQUIRE(price);
        REQUIRE(continuation);
        CHECK_THAT(*price, Catch::Matchers::WithinAbs(*continuation, 0.001));
    }
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

TEST_CASE("Finite-difference low-volatility prices remain bounded")
{
    const auto start = day(2025, 1, 1);
    const auto end = day(2026, 1, 1);
    for (const bool call : {false, true}) {
        const auto type = call ? kiyosi::OptionType::call : kiyosi::OptionType::put;
        const double spot = call ? 105.0 : 95.0;
        const auto option = *kiyosi::make_european_option(type, 100.0, start, end);
        for (const double volatility : {0.001, 0.01}) {
            const auto parameters = *kiyosi::make_bsm_parameters(call ? 0.0 : 0.1, call ? 0.1 : 0.0, volatility);
            const auto context = *kiyosi::make_pricing_context(parameters, spot, start);
            const double analytic = *kiyosi::AnalyticVanillaEngine{}.price(option, context);
            for (const auto scheme : {kiyosi::FiniteDifferenceScheme::explicit_euler,
                                      kiyosi::FiniteDifferenceScheme::implicit_euler,
                                      kiyosi::FiniteDifferenceScheme::crank_nicolson}) {
                CAPTURE(call, volatility, scheme);
                const auto coarse = kiyosi::FiniteDifferenceVanillaEngine{{200, 2000, scheme}}.price(option, context);
                const auto fine = kiyosi::FiniteDifferenceVanillaEngine{{800, 2000, scheme}}.price(option, context);
                REQUIRE(coarse);
                REQUIRE(fine);
                CHECK(*coarse >= 0.0);
                CHECK(*fine >= 0.0);
                CHECK(*coarse <= (call ? spot * std::exp(-0.1) : 100.0 * std::exp(-0.1)));
                CHECK(std::abs(*fine - analytic) < std::abs(*coarse - analytic));
            }
        }
    }
}

TEST_CASE("Explicit finite-difference stability includes drift")
{
    const auto start = day(2025, 1, 1);
    const auto option = *kiyosi::make_european_option(kiyosi::OptionType::put, 100.0, start, day(2026, 1, 1));
    const auto context = *kiyosi::make_pricing_context(*kiyosi::make_bsm_parameters(2.0, 0.0, 0.001), 95.0, start);
    const auto result = kiyosi::FiniteDifferenceVanillaEngine{{200, 200, kiyosi::FiniteDifferenceScheme::explicit_euler}}.price(option, context);
    REQUIRE_FALSE(result);
    CHECK(result.error().category == kiyosi::ErrorCategory::invalid_parameter);
}

} // namespace
