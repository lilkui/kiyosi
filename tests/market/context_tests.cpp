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

TEST_CASE("BSM parameters and asset prices reject non-finite or non-positive values")
{
    auto valid = kiyosi::make_bsm_parameters(0.05, 0.02, 0.2);
    REQUIRE(valid.has_value());
    REQUIRE(valid->risk_free_rate() == 0.05);
    REQUIRE(valid->dividend_yield() == 0.02);
    REQUIRE(valid->volatility() == 0.2);

    REQUIRE(kiyosi::make_bsm_parameters(std::numeric_limits<double>::quiet_NaN(), 0.0, 0.2).error().category ==
            kiyosi::error_category::invalid_rate);
    REQUIRE_FALSE(kiyosi::make_bsm_parameters(0.0, std::numeric_limits<double>::infinity(), 0.2).has_value());
    REQUIRE_FALSE(kiyosi::make_bsm_parameters(0.0, 0.0, 0.0).has_value());
    REQUIRE_FALSE(kiyosi::make_bsm_parameters(0.0, 0.0, std::numeric_limits<double>::infinity()).has_value());

    REQUIRE(kiyosi::make_asset_price(100.0).has_value());
    REQUIRE_FALSE(kiyosi::make_asset_price(0.0).has_value());
    REQUIRE_FALSE(kiyosi::make_asset_price(-1.0).has_value());
    REQUIRE_FALSE(kiyosi::make_asset_price(std::numeric_limits<double>::quiet_NaN()).has_value());
}

TEST_CASE("Pricing context and result preserve their values")
{
    auto parameters = kiyosi::make_bsm_parameters(0.05, 0.02, 0.2);
    auto context = kiyosi::make_pricing_context(*parameters, *kiyosi::make_asset_price(100.0), day(2025, 1, 1));
    REQUIRE(context.has_value());
    REQUIRE(context->asset_price().value() == 100.0);
    REQUIRE(context->valuation_date() == day(2025, 1, 1));
    REQUIRE(kiyosi::make_pricing_context(*parameters, *kiyosi::make_asset_price(100.0), day(2025, 1, 1)).has_value());

    const kiyosi::PricingResult result{{kiyosi::risk_measure::price, 1.0},
                                       {kiyosi::risk_measure::delta, 2.0},
                                       {kiyosi::risk_measure::gamma, 3.0},
                                       {kiyosi::risk_measure::speed, 4.0},
                                       {kiyosi::risk_measure::theta, 5.0},
                                       {kiyosi::risk_measure::charm, 6.0},
                                       {kiyosi::risk_measure::color, 7.0},
                                       {kiyosi::risk_measure::vega, 8.0},
                                       {kiyosi::risk_measure::vanna, 9.0},
                                       {kiyosi::risk_measure::zomma, 10.0},
                                       {kiyosi::risk_measure::rho, 11.0}};
    const auto copy = result;
    REQUIRE(risk_value(copy, kiyosi::risk_measure::rho) == 11.0);
    STATIC_REQUIRE(std::is_copy_constructible_v<kiyosi::PricingResult>);
    STATIC_REQUIRE(std::is_copy_assignable_v<kiyosi::PricingResult>);
    STATIC_REQUIRE(std::is_copy_constructible_v<kiyosi::Error>);
    STATIC_REQUIRE_FALSE(std::is_convertible_v<double, kiyosi::AssetPrice>);
}

}
