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
    CHECK(kiyosi::make_barrier_option(static_cast<kiyosi::option_type>(99), 100.0, effective, expiry, 90.0,
                                      kiyosi::barrier_type::down_and_in)
              .error()
              .category == kiyosi::error_category::invalid_option);
    CHECK(kiyosi::make_barrier_option(kiyosi::option_type::call, -1.0, effective, expiry, 90.0,
                                      kiyosi::barrier_type::down_and_in)
              .error()
              .category == kiyosi::error_category::invalid_strike);
    CHECK(kiyosi::make_barrier_option(kiyosi::option_type::call, 100.0, effective, expiry, -1.0,
                                      kiyosi::barrier_type::down_and_in)
              .error()
              .category == kiyosi::error_category::invalid_parameter);
    CHECK(kiyosi::make_barrier_option(kiyosi::option_type::call, 100.0, effective, expiry, 90.0,
                                      kiyosi::barrier_type::down_and_in, -1.0)
              .error()
              .category == kiyosi::error_category::invalid_parameter);
    CHECK(kiyosi::make_barrier_option(kiyosi::option_type::call, 100.0, effective, expiry, 90.0,
                                      kiyosi::barrier_type::down_and_in, 10.0,
                                      kiyosi::rebate_timing::at_hit)
              .error()
              .category == kiyosi::error_category::invalid_option);
}

TEST_CASE("Barrier option exposes its contractual terms")
{
    const auto effective = day(2025, 1, 6);
    const auto expiry = effective + std::chrono::days{365};
    const auto option = kiyosi::make_barrier_option(kiyosi::option_type::call, 100.0, effective, expiry, 120.0,
                                                     kiyosi::barrier_type::up_and_out, 5.0,
                                                     kiyosi::rebate_timing::at_expiry);
    REQUIRE(option.has_value());
    CHECK(option->type() == kiyosi::option_type::call);
    CHECK(option->strike() == 100.0);
    CHECK(option->rebate() == 5.0);
    CHECK(option->rebate_payment() == kiyosi::rebate_timing::at_expiry);
    CHECK(option->barrier() == 120.0);
    CHECK(option->barrier_kind() == kiyosi::barrier_type::up_and_out);
    CHECK(option->effective() == effective);
    CHECK(option->expiry() == expiry);
}
