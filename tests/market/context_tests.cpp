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
using kiyosi::test::greek_value;

TEST_CASE("Market replace preserves omitted fields and validates copies", "[market-replace]")
{
    const auto parameters = kiyosi::make_bsm_parameters(0.03, 0.01, 0.2);
    REQUIRE(parameters);
    const auto changed = parameters->replace({.volatility = 0.25});
    REQUIRE(changed);
    CHECK(changed->risk_free_rate() == 0.03);
    CHECK(changed->dividend_yield() == 0.01);
    CHECK(changed->volatility() == 0.25);
    CHECK(parameters->replace() == parameters);
    const auto zero_rates = changed->replace({.risk_free_rate = 0, .dividend_yield = 0});
    REQUIRE(zero_rates);
    CHECK(zero_rates->risk_free_rate() == 0);
    CHECK(zero_rates->dividend_yield() == 0);
    CHECK(zero_rates->volatility() == 0.25);

    const auto time = kiyosi::start_of_day(day(2025, 1, 3)) + std::chrono::microseconds{45001123456};
    const auto context = kiyosi::make_pricing_context(*parameters, 100, time, kiyosi::all_days_calendar());
    REQUIRE(context);
    const auto copy = context->replace();
    REQUIRE(copy);
    CHECK(copy->spot_price() == 100);
    CHECK(copy->model_parameters() == *parameters);
    CHECK(copy->valuation_time() == time);
    CHECK(copy->calendar().trading_days_per_year() == 365);
    const auto bumped = context->replace({.spot_price = 105});
    REQUIRE(bumped);
    CHECK(bumped->spot_price() == 105);
    CHECK(bumped->model_parameters() == *parameters);
    CHECK(bumped->valuation_time() == time);
    CHECK(bumped->calendar().is_trading_day(day(2025, 1, 4)));
    const auto new_time = kiyosi::start_of_day(day(2025, 1, 6)) + std::chrono::hours{1};
    const auto replaced = context->replace({.model_parameters = *changed, .spot_price = 110, .valuation_time = new_time, .calendar = kiyosi::weekdays_calendar()});
    REQUIRE(replaced);
    CHECK(replaced->model_parameters() == *changed);
    CHECK(replaced->spot_price() == 110);
    CHECK(replaced->valuation_time() == new_time);
    CHECK_FALSE(replaced->calendar().is_trading_day(day(2025, 1, 4)));

    const auto invalid_rate = parameters->replace({.risk_free_rate = std::numeric_limits<double>::quiet_NaN()});
    REQUIRE_FALSE(invalid_rate);
    CHECK(invalid_rate.error().category == kiyosi::ErrorCategory::invalid_risk_free_rate);
    const auto invalid_yield = parameters->replace({.dividend_yield = std::numeric_limits<double>::infinity()});
    REQUIRE_FALSE(invalid_yield);
    CHECK(invalid_yield.error().category == kiyosi::ErrorCategory::invalid_dividend_yield);
    const auto invalid_volatility = parameters->replace({.volatility = 0});
    REQUIRE_FALSE(invalid_volatility);
    CHECK(invalid_volatility.error().category == kiyosi::ErrorCategory::invalid_volatility);
    const auto invalid_spot = context->replace({.spot_price = 0, .calendar = kiyosi::weekdays_calendar()});
    REQUIRE_FALSE(invalid_spot);
    CHECK(invalid_spot.error().category == kiyosi::ErrorCategory::invalid_spot_price);
    CHECK(parameters->risk_free_rate() == 0.03);
    CHECK(parameters->dividend_yield() == 0.01);
    CHECK(parameters->volatility() == 0.2);
    CHECK(context->model_parameters() == *parameters);
    CHECK(context->spot_price() == 100);
    CHECK(context->valuation_time() == time);
    CHECK(context->calendar().is_trading_day(day(2025, 1, 4)));
}

TEST_CASE("BSM parameters and pricing contexts reject invalid market inputs")
{
    auto valid = kiyosi::make_bsm_parameters(0.05, 0.02, 0.2);
    REQUIRE(valid.has_value());
    REQUIRE(valid->risk_free_rate() == 0.05);
    REQUIRE(valid->dividend_yield() == 0.02);
    REQUIRE(valid->volatility() == 0.2);

    const auto invalid_rate = kiyosi::make_bsm_parameters(std::numeric_limits<double>::quiet_NaN(), 0.0, 0.2);
    REQUIRE_FALSE(invalid_rate.has_value());
    REQUIRE(invalid_rate.error().category ==
            kiyosi::ErrorCategory::invalid_risk_free_rate);
    REQUIRE_FALSE(kiyosi::make_bsm_parameters(0.0, std::numeric_limits<double>::infinity(), 0.2).has_value());
    REQUIRE_FALSE(kiyosi::make_bsm_parameters(0.0, 0.0, 0.0).has_value());
    REQUIRE_FALSE(kiyosi::make_bsm_parameters(0.0, 0.0, std::numeric_limits<double>::infinity()).has_value());

    const auto valuation = day(2025, 1, 1);
    REQUIRE(kiyosi::make_pricing_context(*valid, 100.0, valuation).has_value());
    const auto invalid_spot = kiyosi::make_pricing_context(*valid, 0.0, valuation);
    REQUIRE_FALSE(invalid_spot.has_value());
    REQUIRE(invalid_spot.error().category ==
            kiyosi::ErrorCategory::invalid_spot_price);
    REQUIRE_FALSE(kiyosi::make_pricing_context(*valid, -1.0, valuation).has_value());
    REQUIRE_FALSE(kiyosi::make_pricing_context(
                      *valid, std::numeric_limits<double>::quiet_NaN(), valuation)
                      .has_value());
}

TEST_CASE("Pricing context and result preserve their values")
{
    auto parameters = kiyosi::make_bsm_parameters(0.05, 0.02, 0.2);
    REQUIRE(parameters.has_value());
    auto context = kiyosi::make_pricing_context(*parameters, 100.0, day(2025, 1, 1));
    REQUIRE(context.has_value());
    REQUIRE(context->spot_price() == 100.0);
    REQUIRE(context->valuation_date() == day(2025, 1, 1));
    REQUIRE(kiyosi::make_pricing_context(*parameters, 100.0, day(2025, 1, 1)).has_value());

    const auto result = kiyosi::make_pricing_result(1.0, {{kiyosi::Greek::delta, 2.0},
                                                          {kiyosi::Greek::gamma, 3.0},
                                                          {kiyosi::Greek::speed, 4.0},
                                                          {kiyosi::Greek::theta, 5.0},
                                                          {kiyosi::Greek::charm, 6.0},
                                                          {kiyosi::Greek::color, 7.0},
                                                          {kiyosi::Greek::vega, 8.0},
                                                          {kiyosi::Greek::vanna, 9.0},
                                                          {kiyosi::Greek::zomma, 10.0},
                                                          {kiyosi::Greek::rho, 11.0}});
    REQUIRE(result.has_value());
    CHECK(result->price() == 1.0);
    const auto copy = *result;
    REQUIRE(greek_value(copy, kiyosi::Greek::rho) == 11.0);
    STATIC_REQUIRE(std::is_copy_constructible_v<kiyosi::PricingResult>);
    STATIC_REQUIRE(std::is_copy_assignable_v<kiyosi::PricingResult>);
    STATIC_REQUIRE(std::is_copy_constructible_v<kiyosi::Error>);
}

TEST_CASE("Pricing result rejects unknown Greeks")
{
    const auto unknown = static_cast<kiyosi::Greek>(kiyosi::greek_count);
    const auto result = kiyosi::make_pricing_result(1.0);
    REQUIRE(result.has_value());

    CHECK_FALSE(result->has(unknown));

    const auto unknown_value = result->get(unknown);
    REQUIRE_FALSE(unknown_value.has_value());
    CHECK(unknown_value.error().category == kiyosi::ErrorCategory::invalid_parameter);

    const auto unknown_required = result->require(unknown);
    REQUIRE_FALSE(unknown_required.has_value());
    CHECK(unknown_required.error().category == kiyosi::ErrorCategory::invalid_parameter);

    const auto unavailable = result->get(kiyosi::Greek::delta);
    REQUIRE(unavailable.has_value());
    CHECK_FALSE(unavailable->has_value());
    const auto missing_delta = result->require(kiyosi::Greek::delta);
    REQUIRE_FALSE(missing_delta.has_value());
    CHECK(missing_delta.error().category ==
          kiyosi::ErrorCategory::invalid_result);

    const auto invalid = kiyosi::make_pricing_result(1.0, {{unknown, 1.0}});
    REQUIRE_FALSE(invalid.has_value());
    CHECK(invalid.error().category == kiyosi::ErrorCategory::invalid_parameter);
    STATIC_REQUIRE_FALSE(noexcept(result->get(unknown)));
}

TEST_CASE("Pricing result rejects non-finite prices and available Greeks")
{
    for (const double value : {std::numeric_limits<double>::quiet_NaN(),
                               std::numeric_limits<double>::infinity(),
                               -std::numeric_limits<double>::infinity()}) {
        const auto price = kiyosi::make_pricing_result(value);
        REQUIRE_FALSE(price);
        CHECK(price.error().category == kiyosi::ErrorCategory::invalid_result);
        for (std::size_t index = 0; index < kiyosi::greek_count; ++index) {
            CAPTURE(index);
            const auto greek = static_cast<kiyosi::Greek>(index);
            const auto result = kiyosi::make_pricing_result(1.0, {{greek, value}});
            REQUIRE_FALSE(result);
            CHECK(result.error().category == kiyosi::ErrorCategory::invalid_result);
        }
    }
}

TEST_CASE("Pricing result validates final duplicate entries and preserves unavailable Greeks")
{
    using kiyosi::Greek;
    const double infinity = std::numeric_limits<double>::infinity();
    const auto result = kiyosi::make_pricing_result(-2.0, {{Greek::delta, infinity},
                                                           {Greek::delta, -3.0},
                                                           {Greek::gamma, infinity},
                                                           {Greek::gamma, std::nullopt},
                                                           {Greek::rho, 0.0}});
    REQUIRE(result);
    CHECK(result->price() == -2.0);
    CHECK(greek_value(*result, Greek::delta) == -3.0);
    CHECK_FALSE(result->has(Greek::gamma));
    CHECK_FALSE(result->has(Greek::vega));
    CHECK(greek_value(*result, Greek::rho) == 0.0);

    const auto invalid = kiyosi::make_pricing_result(1.0, {{Greek::delta, 3.0}, {Greek::delta, infinity}});
    REQUIRE_FALSE(invalid);
    CHECK(invalid.error().category == kiyosi::ErrorCategory::invalid_result);
}

} // namespace
