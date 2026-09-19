#include <catch2/catch_test_macros.hpp>

#include <chrono>
#include <variant>

#include <kiyosi/kiyosi.hpp>

#include "support/common.hpp"

namespace {
using kiyosi::test::day;
}

TEST_CASE("Binary barrier factories reject invalid contracts")
{
    const auto effective_date = day(2025, 1, 6);
    const auto expiry_date = effective_date + std::chrono::days{365};
    const auto terms = kiyosi::BinaryBarrierTerms{.option_type = kiyosi::OptionType::put,
                                                  .strike = 100.0,
                                                  .effective_date = effective_date,
                                                  .expiry_date = expiry_date,
                                                  .barrier_level = 90.0,
                                                  .barrier_type = kiyosi::BarrierType::down_and_in};
    auto invalid = terms;
    invalid.option_type = static_cast<kiyosi::OptionType>(99);
    CHECK(kiyosi::make_cash_binary_barrier_option(invalid, 10.0).error().category ==
          kiyosi::ErrorCategory::invalid_option);
    CHECK(kiyosi::make_cash_binary_barrier_option(terms, -1.0).error().category ==
          kiyosi::ErrorCategory::invalid_parameter);
    invalid = terms;
    invalid.observation_mode = kiyosi::ObservationMode::scheduled;
    CHECK(kiyosi::make_asset_binary_barrier_option(invalid).error().category ==
          kiyosi::ErrorCategory::invalid_schedule);
}

TEST_CASE("Binary barrier options expose only strike-product terms")
{
    const auto effective_date = day(2025, 1, 6);
    const auto expiry_date = effective_date + std::chrono::days{365};
    const auto option = kiyosi::make_cash_binary_barrier_option(
        {.option_type = kiyosi::OptionType::put,
         .strike = 100.0,
         .effective_date = effective_date,
         .expiry_date = expiry_date,
         .barrier_level = 90.0,
         .barrier_type = kiyosi::BarrierType::down_and_in},
        10.0);
    REQUIRE(option);
    CHECK(option->option_type() == kiyosi::OptionType::put);
    CHECK(option->strike() == 100.0);
    CHECK(option->payoff_type() == kiyosi::PayoffType::cash);
    CHECK(std::get<kiyosi::CashOrNothingPayoff>(option->payoff()).payout() == 10.0);
    CHECK(option->barrier_level() == 90.0);
    CHECK(option->barrier_type() == kiyosi::BarrierType::down_and_in);
}

TEST_CASE("Touch options have no synthetic strike or option type")
{
    const auto effective_date = day(2025, 1, 6);
    const auto expiry_date = effective_date + std::chrono::days{365};
    const auto cash = kiyosi::make_cash_one_touch_up(
        effective_date, expiry_date, 130.0, 10.0, kiyosi::SettlementTiming::at_hit);
    const auto asset = kiyosi::make_asset_no_touch_down(effective_date, expiry_date, 70.0);
    REQUIRE(cash);
    REQUIRE(asset);
    CHECK(cash->is_one_touch());
    CHECK(cash->is_up());
    CHECK(cash->payoff_type() == kiyosi::PayoffType::cash);
    CHECK(cash->settlement_timing() == kiyosi::SettlementTiming::at_hit);
    CHECK_FALSE(asset->is_one_touch());
    CHECK_FALSE(asset->is_up());
    CHECK(asset->payoff_type() == kiyosi::PayoffType::asset);
    CHECK(asset->settlement_timing() == kiyosi::SettlementTiming::at_expiry);
}
