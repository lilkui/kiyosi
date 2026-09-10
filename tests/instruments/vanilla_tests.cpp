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
    const auto expiry = day(2030, 1, 1);
    auto call = kiyosi::make_european_option(kiyosi::option_type::call, 100.0, expiry);
    REQUIRE(call.has_value());
    REQUIRE(call->type() == kiyosi::option_type::call);
    REQUIRE(call->strike() == 100.0);
    REQUIRE(call->expiry() == expiry);

    const auto copy = *call;
    REQUIRE(copy == *call);
    REQUIRE(kiyosi::make_european_option(kiyosi::option_type::put, 100.0, expiry)->type() == kiyosi::option_type::put);
}

TEST_CASE("European option factories reject invalid terms")
{
    const auto expiry = day(2030, 1, 1);
    REQUIRE(kiyosi::make_european_option(kiyosi::option_type::call, 0.0, expiry).error().category ==
            kiyosi::error_category::invalid_strike);
    REQUIRE(kiyosi::make_european_option(kiyosi::option_type::call, -1.0, expiry).error().message.find("strike") !=
            std::string::npos);
    REQUIRE_FALSE(kiyosi::make_european_option(kiyosi::option_type::call,
                                               std::numeric_limits<double>::infinity(), expiry)
                      .has_value());
    REQUIRE_FALSE(kiyosi::make_european_option(static_cast<kiyosi::option_type>(99), 100.0, expiry).has_value());
    REQUIRE(kiyosi::make_european_option(kiyosi::option_type::call, 100.0, expiry, expiry).has_value());
    REQUIRE_FALSE(kiyosi::make_european_option(kiyosi::option_type::call, 100.0, day(2031, 1, 1), expiry).has_value());
}

TEST_CASE("Exercise style and engine risk measures are explicit")
{
    const auto valuation = day(2025, 1, 1);
    const auto expiry = valuation + std::chrono::days{365};
    const auto parameters = *kiyosi::make_bsm_parameters(0.05, 0.02, 0.2);
    const auto context = *kiyosi::make_pricing_context(parameters, *kiyosi::make_asset_price(100.0), valuation);
    const auto european = *kiyosi::make_european_call(100.0, expiry);
    const auto american = *kiyosi::make_american_call(100.0, expiry);
    const auto european_result = kiyosi::AnalyticEuropeanEngine{}.price(european, context);
    REQUIRE(european_result.has_value());
    CHECK(european_result->has(kiyosi::risk_measure::price));
    CHECK(european_result->has(kiyosi::risk_measure::vega));

    const auto american_result = kiyosi::BinomialAmericanEngine{}.price(american, context);
    REQUIRE(american_result.has_value());
    CHECK(american_result->has(kiyosi::risk_measure::price));
    CHECK(american_result->has(kiyosi::risk_measure::gamma));
    CHECK_FALSE(american_result->has(kiyosi::risk_measure::vega));

    const auto invalid_measure = static_cast<kiyosi::risk_measure>(255);
    CHECK_FALSE(european_result->has(invalid_measure));
    CHECK_FALSE(european_result->get(invalid_measure).has_value());
}

}
