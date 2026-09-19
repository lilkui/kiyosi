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

TEST_CASE("BSM parameters and pricing contexts reject invalid market inputs")
{
    auto valid = kiyosi::make_bsm_parameters(0.05, 0.02, 0.2);
    REQUIRE(valid.has_value());
    REQUIRE(valid->risk_free_rate() == 0.05);
    REQUIRE(valid->dividend_yield() == 0.02);
    REQUIRE(valid->volatility() == 0.2);

    REQUIRE(kiyosi::make_bsm_parameters(std::numeric_limits<double>::quiet_NaN(), 0.0, 0.2).error().category ==
            kiyosi::ErrorCategory::invalid_risk_free_rate);
    REQUIRE_FALSE(kiyosi::make_bsm_parameters(0.0, std::numeric_limits<double>::infinity(), 0.2).has_value());
    REQUIRE_FALSE(kiyosi::make_bsm_parameters(0.0, 0.0, 0.0).has_value());
    REQUIRE_FALSE(kiyosi::make_bsm_parameters(0.0, 0.0, std::numeric_limits<double>::infinity()).has_value());

    const auto valuation = day(2025, 1, 1);
    REQUIRE(kiyosi::make_pricing_context(*valid, 100.0, valuation).has_value());
    REQUIRE(kiyosi::make_pricing_context(*valid, 0.0, valuation).error().category ==
            kiyosi::ErrorCategory::invalid_spot_price);
    REQUIRE_FALSE(kiyosi::make_pricing_context(*valid, -1.0, valuation).has_value());
    REQUIRE_FALSE(kiyosi::make_pricing_context(
        *valid, std::numeric_limits<double>::quiet_NaN(), valuation).has_value());
}

TEST_CASE("Pricing context and result preserve their values")
{
    auto parameters = kiyosi::make_bsm_parameters(0.05, 0.02, 0.2);
    auto context = kiyosi::make_pricing_context(*parameters, 100.0, day(2025, 1, 1));
    REQUIRE(context.has_value());
    REQUIRE(context->spot_price() == 100.0);
    REQUIRE(context->valuation_date() == day(2025, 1, 1));
    REQUIRE(kiyosi::make_pricing_context(*parameters, 100.0, day(2025, 1, 1)).has_value());

    const auto result = kiyosi::make_pricing_result({{kiyosi::RiskMeasure::price, 1.0},
                                                     {kiyosi::RiskMeasure::delta, 2.0},
                                                     {kiyosi::RiskMeasure::gamma, 3.0},
                                                     {kiyosi::RiskMeasure::speed, 4.0},
                                                     {kiyosi::RiskMeasure::theta, 5.0},
                                                     {kiyosi::RiskMeasure::charm, 6.0},
                                                     {kiyosi::RiskMeasure::color, 7.0},
                                                     {kiyosi::RiskMeasure::vega, 8.0},
                                                     {kiyosi::RiskMeasure::vanna, 9.0},
                                                     {kiyosi::RiskMeasure::zomma, 10.0},
                                                     {kiyosi::RiskMeasure::rho, 11.0}});
    REQUIRE(result.has_value());
    const auto copy = *result;
    REQUIRE(risk_value(copy, kiyosi::RiskMeasure::rho) == 11.0);
    STATIC_REQUIRE(std::is_copy_constructible_v<kiyosi::PricingResult>);
    STATIC_REQUIRE(std::is_copy_assignable_v<kiyosi::PricingResult>);
    STATIC_REQUIRE(std::is_copy_constructible_v<kiyosi::Error>);
}

TEST_CASE("Pricing result rejects unknown risk measures")
{
    const auto unknown = static_cast<kiyosi::RiskMeasure>(kiyosi::risk_measure_count);
    const auto result = kiyosi::make_pricing_result({{kiyosi::RiskMeasure::price, 1.0}});
    REQUIRE(result.has_value());

    CHECK_FALSE(result->has(unknown));

    const auto unknown_value = result->get(unknown);
    REQUIRE_FALSE(unknown_value.has_value());
    CHECK(unknown_value.error().category == kiyosi::ErrorCategory::invalid_parameter);

    const auto unknown_required = result->require(unknown);
    REQUIRE_FALSE(unknown_required.has_value());
    CHECK(unknown_required.error().category == kiyosi::ErrorCategory::invalid_parameter);

    const auto unavailable = result->get(kiyosi::RiskMeasure::delta);
    REQUIRE(unavailable.has_value());
    CHECK_FALSE(unavailable->has_value());
    CHECK(result->require(kiyosi::RiskMeasure::delta).error().category ==
          kiyosi::ErrorCategory::invalid_result);

    const auto invalid = kiyosi::make_pricing_result({{unknown, 1.0}});
    REQUIRE_FALSE(invalid.has_value());
    CHECK(invalid.error().category == kiyosi::ErrorCategory::invalid_parameter);
    STATIC_REQUIRE_FALSE(noexcept(result->get(unknown)));
}

}
