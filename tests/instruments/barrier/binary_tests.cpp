#include <catch2/catch_test_macros.hpp>

#include <chrono>
#include <optional>

#include <kiyosi/kiyosi.hpp>

#include "support/common.hpp"

namespace {
using kiyosi::test::day;
}

TEST_CASE("Binary barrier option factory rejects invalid contracts")
{
    const auto effective = day(2025, 1, 6);
    const auto expiry = effective + std::chrono::days{365};
    CHECK(kiyosi::make_binary_barrier_option(std::optional{static_cast<kiyosi::option_type>(99)}, 100.0, effective,
                                             expiry, 90.0, kiyosi::barrier_type::down_and_in, 10.0)
              .error()
              .category == kiyosi::error_category::invalid_option);
    CHECK(kiyosi::make_binary_barrier_option(std::nullopt, 100.0, effective, expiry, 90.0,
                                             kiyosi::barrier_type::down_and_out, -1.0)
              .error()
              .category == kiyosi::error_category::invalid_parameter);
    CHECK(kiyosi::make_binary_barrier_option(std::nullopt, 100.0, effective, expiry, 90.0,
                                             kiyosi::barrier_type::down_and_out, 10.0, false,
                                             kiyosi::rebate_timing::at_hit)
              .error()
              .category == kiyosi::error_category::invalid_option);
    CHECK(kiyosi::make_binary_barrier_option(std::nullopt, 100.0, effective, expiry, 90.0,
                                             kiyosi::barrier_type::down_and_in, 10.0, false,
                                             kiyosi::rebate_timing::at_expiry,
                                             kiyosi::observation_mode::scheduled)
              .error()
              .category == kiyosi::error_category::invalid_schedule);
}

TEST_CASE("Binary barrier option exposes its contractual terms")
{
    const auto effective = day(2025, 1, 6);
    const auto expiry = effective + std::chrono::days{365};
    const auto option = kiyosi::make_binary_barrier_option(
        std::optional{kiyosi::option_type::put}, 100.0, effective, expiry, 90.0,
        kiyosi::barrier_type::down_and_in, 10.0);
    REQUIRE(option.has_value());
    CHECK(option->type() == std::optional{kiyosi::option_type::put});
    CHECK(option->strike() == 100.0);
    CHECK(option->payout() == 10.0);
    CHECK_FALSE(option->asset_settlement());
    CHECK(option->barrier() == 90.0);
    CHECK(option->barrier_kind() == kiyosi::barrier_type::down_and_in);
}
