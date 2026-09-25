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
