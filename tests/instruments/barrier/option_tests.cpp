#include <catch2/catch_test_macros.hpp>

#include <chrono>

#include <kiyosi/kiyosi.hpp>

#include "support/common.hpp"

namespace {
using kiyosi::test::day;
}

TEST_CASE("Barrier option factory rejects invalid contracts")
{
    const auto effective = day(2025, 1, 6);
    const auto expiry = effective + std::chrono::days{365};
    const auto terms = kiyosi::BarrierOptionTerms{.type = kiyosi::option_type::call,
                                                  .strike = 100.0,
                                                  .effective = effective,
                                                  .expiry = expiry,
                                                  .barrier = 90.0,
                                                  .barrier_kind = kiyosi::barrier_type::down_and_in};
    auto invalid = terms;
    invalid.type = static_cast<kiyosi::option_type>(99);
    CHECK(kiyosi::make_barrier_option(invalid).error().category == kiyosi::error_category::invalid_option);
    invalid = terms;
    invalid.strike = -1.0;
    CHECK(kiyosi::make_barrier_option(invalid).error().category == kiyosi::error_category::invalid_strike);
    invalid = terms;
    invalid.barrier = -1.0;
    CHECK(kiyosi::make_barrier_option(invalid).error().category == kiyosi::error_category::invalid_parameter);
    invalid = terms;
    invalid.rebate = -1.0;
    CHECK(kiyosi::make_barrier_option(invalid).error().category == kiyosi::error_category::invalid_parameter);
    invalid = terms;
    invalid.rebate = 10.0;
    invalid.rebate_timing = kiyosi::rebate_timing::at_hit;
    CHECK(kiyosi::make_barrier_option(invalid).error().category == kiyosi::error_category::invalid_option);
}

TEST_CASE("Barrier option exposes its contractual terms")
{
    const auto effective = day(2025, 1, 6);
    const auto expiry = effective + std::chrono::days{365};
    const auto option = kiyosi::make_barrier_option({.type = kiyosi::option_type::call,
                                                     .strike = 100.0,
                                                     .effective = effective,
                                                     .expiry = expiry,
                                                     .barrier = 120.0,
                                                     .barrier_kind = kiyosi::barrier_type::up_and_out,
                                                     .rebate = 5.0,
                                                     .rebate_timing = kiyosi::rebate_timing::at_expiry});
    REQUIRE(option.has_value());
    CHECK(option->type() == kiyosi::option_type::call);
    CHECK(option->strike() == 100.0);
    CHECK(option->rebate() == 5.0);
    CHECK(option->rebate_timing() == kiyosi::rebate_timing::at_expiry);
    CHECK(option->barrier() == 120.0);
    CHECK(option->barrier_kind() == kiyosi::barrier_type::up_and_out);
    CHECK(option->effective() == effective);
    CHECK(option->expiry() == expiry);
}
