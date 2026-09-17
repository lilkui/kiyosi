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
    const auto expiry = valuation + std::chrono::days{365};
    const auto observations = std::vector<kiyosi::date>{valuation + std::chrono::days{30},
                                                        valuation + std::chrono::days{180}};
    const auto binary = kiyosi::make_cash_binary_barrier_option(
        {.type = kiyosi::option_type::call,
         .strike = 100.0,
         .effective = valuation,
         .expiry = expiry,
         .barrier = 90.0,
         .barrier_kind = kiyosi::barrier_type::down_and_in,
         .observation = kiyosi::observation_mode::scheduled,
         .observations = observations},
        10.0);
    REQUIRE(binary);
    CHECK_THAT(binary->observation_interval(),
               Catch::Matchers::WithinAbs(180.0 / 365.0 / 2.0, 1e-12));
}

TEST_CASE("Touch factories require only payoff-relevant terms")
{
    const auto effective = day(2025, 1, 6);
    const auto expiry = effective + std::chrono::days{365};
    const auto observations = std::vector<kiyosi::date>{effective + std::chrono::days{30}, expiry};
    const auto cash = kiyosi::make_cash_one_touch_up(
        effective, expiry, 130.0, 10.0, kiyosi::settlement_timing::at_hit,
        kiyosi::observation_mode::scheduled, observations);
    const auto asset = kiyosi::make_asset_no_touch_down(effective, expiry, 70.0);
    REQUIRE(cash);
    REQUIRE(asset);
    CHECK(cash->is_one_touch());
    CHECK(cash->is_up());
    CHECK(cash->payoff_kind() == kiyosi::payoff_type::cash);
    CHECK(cash->settlement() == kiyosi::settlement_timing::at_hit);
    CHECK(cash->observation() == kiyosi::observation_mode::scheduled);
    CHECK(cash->observation_dates() == observations);
    CHECK_FALSE(asset->is_one_touch());
    CHECK_FALSE(asset->is_up());
    CHECK(asset->payoff_kind() == kiyosi::payoff_type::asset);
    CHECK(asset->settlement() == kiyosi::settlement_timing::at_expiry);
}
