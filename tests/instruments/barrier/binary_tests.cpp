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
    const auto effective = day(2025, 1, 6);
    const auto expiry = effective + std::chrono::days{365};
    const auto terms = kiyosi::BinaryBarrierTerms{.type = kiyosi::option_type::put,
                                                  .strike = 100.0,
                                                  .effective = effective,
                                                  .expiry = expiry,
                                                  .barrier = 90.0,
                                                  .barrier_kind = kiyosi::barrier_type::down_and_in};
    auto invalid = terms;
    invalid.type = static_cast<kiyosi::option_type>(99);
    CHECK(kiyosi::make_cash_binary_barrier_option(invalid, 10.0).error().category ==
          kiyosi::error_category::invalid_option);
    CHECK(kiyosi::make_cash_binary_barrier_option(terms, -1.0).error().category ==
          kiyosi::error_category::invalid_parameter);
    invalid = terms;
    invalid.observation_mode = kiyosi::observation_mode::scheduled;
    CHECK(kiyosi::make_asset_binary_barrier_option(invalid).error().category ==
          kiyosi::error_category::invalid_schedule);
}

TEST_CASE("Binary barrier options expose only strike-product terms")
{
    const auto effective = day(2025, 1, 6);
    const auto expiry = effective + std::chrono::days{365};
    const auto option = kiyosi::make_cash_binary_barrier_option(
        {.type = kiyosi::option_type::put,
         .strike = 100.0,
         .effective = effective,
         .expiry = expiry,
         .barrier = 90.0,
         .barrier_kind = kiyosi::barrier_type::down_and_in},
        10.0);
    REQUIRE(option);
    CHECK(option->type() == kiyosi::option_type::put);
    CHECK(option->strike() == 100.0);
    CHECK(option->payoff_kind() == kiyosi::payoff_type::cash);
    CHECK(std::get<kiyosi::CashOrNothingPayoff>(option->payoff()).payout() == 10.0);
    CHECK(option->barrier() == 90.0);
    CHECK(option->barrier_kind() == kiyosi::barrier_type::down_and_in);
}

TEST_CASE("Touch options have no synthetic strike or option type")
{
    const auto effective = day(2025, 1, 6);
    const auto expiry = effective + std::chrono::days{365};
    const auto cash = kiyosi::make_cash_one_touch_up(
        effective, expiry, 130.0, 10.0, kiyosi::settlement_timing::at_hit);
    const auto asset = kiyosi::make_asset_no_touch_down(effective, expiry, 70.0);
    REQUIRE(cash);
    REQUIRE(asset);
    CHECK(cash->is_one_touch());
    CHECK(cash->is_up());
    CHECK(cash->payoff_kind() == kiyosi::payoff_type::cash);
    CHECK(cash->settlement_timing() == kiyosi::settlement_timing::at_hit);
    CHECK_FALSE(asset->is_one_touch());
    CHECK_FALSE(asset->is_up());
    CHECK(asset->payoff_kind() == kiyosi::payoff_type::asset);
    CHECK(asset->settlement_timing() == kiyosi::settlement_timing::at_expiry);
}
