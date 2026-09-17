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

TEST_CASE("Binary barriers expose observation intervals and reject invalid at-hit terms")
{
    const auto valuation = day(2025, 1, 6);
    const auto expiry = valuation + std::chrono::days{365};
    const auto observations = std::vector<kiyosi::date>{valuation + std::chrono::days{30}, valuation + std::chrono::days{180}};
    const auto binary = kiyosi::make_binary_barrier_option({.type = kiyosi::option_type::call,
                                                            .strike = 100.0,
                                                            .effective = valuation,
                                                            .expiry = expiry,
                                                            .barrier = 90.0,
                                                            .barrier_kind = kiyosi::barrier_type::down_and_in,
                                                            .payout = 10.0,
                                                            .asset_settlement = false,
                                                            .settlement_timing = kiyosi::rebate_timing::at_expiry,
                                                            .observation = kiyosi::observation_mode::scheduled,
                                                            .observations = observations});
    REQUIRE(binary.has_value());
    CHECK_THAT(binary->observation_interval(), Catch::Matchers::WithinAbs(180.0 / 365.0 / 2.0, 1e-12));
    CHECK(kiyosi::make_binary_barrier_option({.type = kiyosi::option_type::call,
                                              .strike = 100.0,
                                              .effective = valuation,
                                              .expiry = expiry,
                                              .barrier = 90.0,
                                              .barrier_kind = kiyosi::barrier_type::down_and_out,
                                              .payout = 10.0,
                                              .asset_settlement = false,
                                              .settlement_timing = kiyosi::rebate_timing::at_hit})
              .error()
              .category == kiyosi::error_category::invalid_option);
    CHECK(kiyosi::make_binary_barrier_option({.type = std::nullopt,
                                              .strike = 100.0,
                                              .effective = valuation,
                                              .expiry = expiry,
                                              .barrier = 90.0,
                                              .barrier_kind = kiyosi::barrier_type::down_and_in,
                                              .payout = 89.0,
                                              .asset_settlement = true,
                                              .settlement_timing = kiyosi::rebate_timing::at_hit})
              .error()
              .category == kiyosi::error_category::invalid_parameter);
}

TEST_CASE("Touch factories require only payoff-relevant terms")
{
    const auto effective = day(2025, 1, 6);
    const auto expiry = effective + std::chrono::days{365};
    const auto cash = kiyosi::make_cash_one_touch_up(
        effective, expiry, 130.0, 10.0, kiyosi::rebate_timing::at_hit);
    const auto asset = kiyosi::make_asset_no_touch_down(effective, expiry, 70.0);
    REQUIRE(cash);
    REQUIRE(asset);
    CHECK(cash->barrier_kind() == kiyosi::barrier_type::up_and_in);
    CHECK(cash->payout() == 10.0);
    CHECK_FALSE(cash->asset_settlement());
    CHECK(cash->settlement_timing() == kiyosi::rebate_timing::at_hit);
    CHECK(cash->observation() == kiyosi::observation_mode::continuous);
    CHECK(asset->barrier_kind() == kiyosi::barrier_type::down_and_out);
    CHECK(asset->asset_settlement());
    CHECK(asset->settlement_timing() == kiyosi::rebate_timing::at_expiry);
}

} // namespace
