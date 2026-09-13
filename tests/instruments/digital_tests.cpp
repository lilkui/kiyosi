#include <catch2/catch_test_macros.hpp>

#include <chrono>

#include <kiyosi/kiyosi.hpp>

#include "support/common.hpp"

namespace {
using kiyosi::test::day;
}

TEST_CASE("Cash-or-nothing and asset-or-nothing options expose their terms and payoff")
{
    const auto effective = day(2025, 1, 6);
    const auto expiry = effective + std::chrono::days{365};

    const auto cash = kiyosi::make_cash_or_nothing_option(kiyosi::option_type::call, 100.0, 10.0, effective, expiry);
    REQUIRE(cash.has_value());
    CHECK(cash->type() == kiyosi::option_type::call);
    CHECK(cash->strike() == 100.0);
    CHECK(cash->payout() == 10.0);
    CHECK(cash->exercise() == kiyosi::EuropeanExercise{});

    const auto asset = kiyosi::make_asset_or_nothing_option(kiyosi::option_type::put, 100.0, effective, expiry);
    REQUIRE(asset.has_value());
    CHECK(asset->type() == kiyosi::option_type::put);
    CHECK(asset->strike() == 100.0);
}

TEST_CASE("Digital option factories reject invalid contracts")
{
    const auto effective = day(2025, 1, 6);
    const auto expiry = effective + std::chrono::days{365};
    const auto invalid_type = static_cast<kiyosi::option_type>(99);

    CHECK(kiyosi::make_cash_or_nothing_option(invalid_type, 100.0, 10.0, effective, expiry).error().category ==
          kiyosi::error_category::invalid_option);
    CHECK(kiyosi::make_asset_or_nothing_option(invalid_type, 100.0, effective, expiry).error().category ==
          kiyosi::error_category::invalid_option);
    CHECK(kiyosi::make_cash_or_nothing_option(kiyosi::option_type::call, 0.0, 10.0, effective, expiry)
              .error()
              .category == kiyosi::error_category::invalid_strike);
    CHECK(kiyosi::make_asset_or_nothing_option(kiyosi::option_type::call, 0.0, effective, expiry)
              .error()
              .category == kiyosi::error_category::invalid_strike);
    CHECK(kiyosi::make_cash_or_nothing_option(kiyosi::option_type::call, 100.0, 0.0, effective, expiry)
              .error()
              .category == kiyosi::error_category::invalid_parameter);
}
