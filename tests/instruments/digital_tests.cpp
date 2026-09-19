#include <catch2/catch_test_macros.hpp>

#include <chrono>

#include <kiyosi/kiyosi.hpp>

#include "support/common.hpp"

namespace {
using kiyosi::test::day;
}

TEST_CASE("Cash-or-nothing and asset-or-nothing options expose their terms and payoff")
{
    const auto effective_date = day(2025, 1, 6);
    const auto expiry_date = effective_date + std::chrono::days{365};

    const auto cash = kiyosi::make_cash_or_nothing_option(kiyosi::OptionType::call, 100.0, 10.0, effective_date, expiry_date);
    REQUIRE(cash.has_value());
    CHECK(cash->option_type() == kiyosi::OptionType::call);
    CHECK(cash->strike() == 100.0);
    CHECK(cash->payout() == 10.0);
    CHECK(cash->exercise() == kiyosi::EuropeanExercise{});

    const auto asset = kiyosi::make_asset_or_nothing_option(kiyosi::OptionType::put, 100.0, effective_date, expiry_date);
    REQUIRE(asset.has_value());
    CHECK(asset->option_type() == kiyosi::OptionType::put);
    CHECK(asset->strike() == 100.0);
}

TEST_CASE("Digital option factories reject invalid contracts")
{
    const auto effective_date = day(2025, 1, 6);
    const auto expiry_date = effective_date + std::chrono::days{365};
    const auto invalid_type = static_cast<kiyosi::OptionType>(99);

    CHECK(kiyosi::make_cash_or_nothing_option(invalid_type, 100.0, 10.0, effective_date, expiry_date).error().category ==
          kiyosi::ErrorCategory::invalid_option);
    CHECK(kiyosi::make_asset_or_nothing_option(invalid_type, 100.0, effective_date, expiry_date).error().category ==
          kiyosi::ErrorCategory::invalid_option);
    CHECK(kiyosi::make_cash_or_nothing_option(kiyosi::OptionType::call, 0.0, 10.0, effective_date, expiry_date)
              .error()
              .category == kiyosi::ErrorCategory::invalid_strike);
    CHECK(kiyosi::make_asset_or_nothing_option(kiyosi::OptionType::call, 0.0, effective_date, expiry_date)
              .error()
              .category == kiyosi::ErrorCategory::invalid_strike);
    CHECK(kiyosi::make_cash_or_nothing_option(kiyosi::OptionType::call, 100.0, 0.0, effective_date, expiry_date)
              .error()
              .category == kiyosi::ErrorCategory::invalid_parameter);
}
