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
    const auto terms = kiyosi::BinaryBarrierTerms{.type = std::nullopt,
                                                  .strike = 100.0,
                                                  .effective = effective,
                                                  .expiry = expiry,
                                                  .barrier = 90.0,
                                                  .barrier_kind = kiyosi::barrier_type::down_and_in,
                                                  .payout = 10.0};
    auto invalid = terms;
    invalid.type = static_cast<kiyosi::option_type>(99);
    CHECK(kiyosi::make_binary_barrier_option(invalid).error().category == kiyosi::error_category::invalid_option);
    invalid = terms;
    invalid.payout = -1.0;
    CHECK(kiyosi::make_binary_barrier_option(invalid).error().category == kiyosi::error_category::invalid_parameter);
    invalid = terms;
    invalid.barrier_kind = kiyosi::barrier_type::down_and_out;
    invalid.settlement_timing = kiyosi::rebate_timing::at_hit;
    CHECK(kiyosi::make_binary_barrier_option(invalid).error().category == kiyosi::error_category::invalid_option);
    invalid = terms;
    invalid.observation = kiyosi::observation_mode::scheduled;
    CHECK(kiyosi::make_binary_barrier_option(invalid).error().category == kiyosi::error_category::invalid_schedule);
}

TEST_CASE("Binary barrier option exposes its contractual terms")
{
    const auto effective = day(2025, 1, 6);
    const auto expiry = effective + std::chrono::days{365};
    const auto option = kiyosi::make_binary_barrier_option({.type = kiyosi::option_type::put,
                                                            .strike = 100.0,
                                                            .effective = effective,
                                                            .expiry = expiry,
                                                            .barrier = 90.0,
                                                            .barrier_kind = kiyosi::barrier_type::down_and_in,
                                                            .payout = 10.0});
    REQUIRE(option.has_value());
    CHECK(option->type() == std::optional{kiyosi::option_type::put});
    CHECK(option->strike() == 100.0);
    CHECK(option->payout() == 10.0);
    CHECK_FALSE(option->asset_settlement());
    CHECK(option->barrier() == 90.0);
    CHECK(option->barrier_kind() == kiyosi::barrier_type::down_and_in);
}
