#include <catch2/catch_test_macros.hpp>
#include <catch2/matchers/catch_matchers_floating_point.hpp>

#include <chrono>
#include <vector>

#include <kiyosi/kiyosi.hpp>

#include "support/common.hpp"

namespace {

using kiyosi::test::day;

}

TEST_CASE("Binary barriers expose observation intervals")
{
    const auto valuation = day(2025, 1, 6);
    const auto expiry_date = valuation + std::chrono::days{365};
    const auto observation_dates = std::vector<kiyosi::Date>{valuation + std::chrono::days{30},
                                                        valuation + std::chrono::days{180}};
    const auto binary = kiyosi::make_cash_binary_barrier_option(
        {.option_type = kiyosi::OptionType::call,
         .strike = 100.0,
         .effective_date = valuation,
         .expiry_date = expiry_date,
         .barrier_level = 90.0,
         .barrier_type = kiyosi::BarrierType::down_and_in,
         .observation_mode = kiyosi::ObservationMode::scheduled,
         .observation_dates = observation_dates},
        10.0);
    REQUIRE(binary);
    CHECK_THAT(binary->mean_observation_year_fraction(),
               Catch::Matchers::WithinAbs(180.0 / 365.0 / 2.0, 1e-12));
}

TEST_CASE("Touch factories require only payoff-relevant terms")
{
    const auto effective_date = day(2025, 1, 6);
    const auto expiry_date = effective_date + std::chrono::days{365};
    const auto observation_dates = std::vector<kiyosi::Date>{effective_date + std::chrono::days{30}, expiry_date};
    const auto cash = kiyosi::make_cash_one_touch_up(
        effective_date, expiry_date, 130.0, 10.0, kiyosi::SettlementTiming::at_hit,
        kiyosi::ObservationMode::scheduled, observation_dates);
    const auto asset = kiyosi::make_asset_no_touch_down(effective_date, expiry_date, 70.0);
    REQUIRE(cash);
    REQUIRE(asset);
    CHECK(cash->is_one_touch());
    CHECK(cash->is_up());
    CHECK(cash->payoff_type() == kiyosi::PayoffType::cash);
    CHECK(cash->settlement_timing() == kiyosi::SettlementTiming::at_hit);
    CHECK(cash->observation_mode() == kiyosi::ObservationMode::scheduled);
    CHECK(cash->observation_dates() == observation_dates);
    CHECK_FALSE(asset->is_one_touch());
    CHECK_FALSE(asset->is_up());
    CHECK(asset->payoff_type() == kiyosi::PayoffType::asset);
    CHECK(asset->settlement_timing() == kiyosi::SettlementTiming::at_expiry);
}
