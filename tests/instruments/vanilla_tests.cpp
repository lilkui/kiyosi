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

TEST_CASE("European options are validated immutable values")
{
    const auto expiry_date = day(2030, 1, 1);
    const auto effective_date = day(2029, 1, 1);
    auto call = kiyosi::make_european_option(kiyosi::OptionType::call, 100.0, effective_date, expiry_date);
    REQUIRE(call.has_value());
    REQUIRE(call->option_type() == kiyosi::OptionType::call);
    REQUIRE(call->strike() == 100.0);
    REQUIRE(call->expiry_date() == expiry_date);

    const auto copy = *call;
    REQUIRE(copy == *call);
    REQUIRE(kiyosi::make_european_option(
                kiyosi::OptionType::put, 100.0, effective_date, expiry_date)
                ->option_type() == kiyosi::OptionType::put);
}

TEST_CASE("European option factories reject invalid terms")
{
    const auto expiry_date = day(2030, 1, 1);
    const auto effective_date = day(2029, 1, 1);
    REQUIRE(kiyosi::make_european_option(kiyosi::OptionType::call, 0.0, effective_date, expiry_date).error().category ==
            kiyosi::ErrorCategory::invalid_strike);
    REQUIRE(kiyosi::make_european_option(kiyosi::OptionType::call, -1.0, effective_date, expiry_date).error().message.find("strike") !=
            std::string::npos);
    REQUIRE_FALSE(kiyosi::make_european_option(kiyosi::OptionType::call,
                                               std::numeric_limits<double>::infinity(), effective_date, expiry_date)
                      .has_value());
    REQUIRE_FALSE(kiyosi::make_european_option(
                      static_cast<kiyosi::OptionType>(99), 100.0, effective_date, expiry_date)
                      .has_value());
    REQUIRE(kiyosi::make_european_option(kiyosi::OptionType::call, 100.0, expiry_date, expiry_date).has_value());
    REQUIRE_FALSE(kiyosi::make_european_option(kiyosi::OptionType::call, 100.0, day(2031, 1, 1), expiry_date).has_value());
}

TEST_CASE("Contract factories distinguish invalid dates from reversed lives")
{
    const auto earlier = day(2025, 1, 1);
    const auto later = day(2026, 1, 1);
    const auto check = [&](auto make) {
        const auto reversed = make(later, earlier);
        REQUIRE_FALSE(reversed);
        CHECK(reversed.error().category == kiyosi::ErrorCategory::invalid_time_range);
        const auto unsupported = make(kiyosi::Date::max(), earlier);
        REQUIRE_FALSE(unsupported);
        CHECK(unsupported.error().category == kiyosi::ErrorCategory::invalid_date);
    };
    check([](auto start, auto end) {
        return kiyosi::make_european_option(kiyosi::OptionType::call, 100.0, start, end);
    });
    check([](auto start, auto end) {
        return kiyosi::make_arithmetic_average_option(kiyosi::OptionType::call, 100.0, start, start, end);
    });
    check([](auto start, auto end) {
        return kiyosi::make_barrier_option({.option_type = kiyosi::OptionType::call, .strike = 100.0,
            .effective_date = start, .expiry_date = end, .barrier_level = 120.0,
            .barrier_type = kiyosi::BarrierType::up_and_out});
    });
    check([](auto start, auto end) {
        return kiyosi::make_accumulator({.strike = 100.0, .knock_out_level = 110.0,
            .daily_quantity = 1.0, .acceleration_factor = 2.0,
            .effective_date = start, .expiry_date = end});
    });
    check([](auto start, auto end) {
        return kiyosi::make_binary_snowball_option({.knock_out_coupon_rates = {0.1},
            .maturity_coupon_rate = 0.05, .initial_spot = 100.0, .knock_out_levels = {110.0},
            .upper_strike = 100.0, .lower_strike = 60.0, .observation_dates = {end},
            .effective_date = start, .expiry_date = end});
    });

    CHECK(kiyosi::validate_date_schedule({&later, 1}, later, earlier).error().category ==
          kiyosi::ErrorCategory::invalid_time_range);
}

TEST_CASE("Exercise style and engine risk measures are explicit")
{
    const auto valuation = day(2025, 1, 1);
    const auto expiry_date = valuation + std::chrono::days{365};
    const auto parameters = *kiyosi::make_bsm_parameters(0.05, 0.02, 0.2);
    const auto context = *kiyosi::make_pricing_context(parameters, 100.0, valuation);
    const auto european = *kiyosi::make_european_option(
        kiyosi::OptionType::call, 100.0, valuation, expiry_date);
    const auto american = *kiyosi::make_american_option(
        kiyosi::OptionType::call, 100.0, valuation, expiry_date);
    const auto european_result = kiyosi::AnalyticVanillaEngine{}.price_with_greeks(european, context, kiyosi::GreeksLevel::full);
    REQUIRE(european_result.has_value());
    CHECK(european_result->has(kiyosi::RiskMeasure::price));
    CHECK(european_result->has(kiyosi::RiskMeasure::vega));

    const auto american_result = kiyosi::CoxRossRubinsteinVanillaEngine{}.price_with_greeks(american, context, kiyosi::GreeksLevel::basic);
    REQUIRE(american_result.has_value());
    CHECK(american_result->has(kiyosi::RiskMeasure::price));
    CHECK(american_result->has(kiyosi::RiskMeasure::gamma));
    CHECK_FALSE(american_result->has(kiyosi::RiskMeasure::vega));
}

} // namespace
