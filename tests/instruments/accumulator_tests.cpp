#include <catch2/catch_test_macros.hpp>

#include <chrono>
#include <type_traits>

#include <kiyosi/kiyosi.hpp>

#include "support/common.hpp"

namespace {
using kiyosi::test::day;
}

TEST_CASE("Accumulator factory rejects invalid contracts")
{
    static_assert(!std::is_constructible_v<kiyosi::Accumulator, double, double, double, double, double,
                                           kiyosi::date, kiyosi::date>);

    const auto effective = day(2025, 1, 1);
    const auto expiry = day(2026, 1, 1);
    CHECK(kiyosi::make_accumulator(0.0, 110.0, 1.0, 2.0, 0.0, effective, expiry).error().category ==
          kiyosi::error_category::invalid_parameter);
    CHECK(kiyosi::make_accumulator(100.0, 0.0, 1.0, 2.0, 0.0, effective, expiry).error().category ==
          kiyosi::error_category::invalid_parameter);
    CHECK(kiyosi::make_accumulator(100.0, 110.0, -1.0, 2.0, 0.0, effective, expiry).error().category ==
          kiyosi::error_category::invalid_parameter);
    CHECK(kiyosi::make_accumulator(100.0, 110.0, 1.0, -1.0, 0.0, effective, expiry).error().category ==
          kiyosi::error_category::invalid_parameter);
    CHECK(kiyosi::make_accumulator(100.0, 110.0, 1.0, 2.0, -1.0, effective, expiry).error().category ==
          kiyosi::error_category::invalid_parameter);
    CHECK(kiyosi::make_accumulator(100.0, 110.0, 1.0, 2.0, 0.0, expiry, effective).error().category ==
          kiyosi::error_category::invalid_schedule);
}

TEST_CASE("Accumulator exposes its contractual terms")
{
    const auto effective = day(2025, 1, 1);
    const auto expiry = day(2026, 1, 1);
    const auto accumulator =
        kiyosi::make_accumulator(100.0, 110.0, 1.0, 2.0, 3.0, effective, expiry);
    REQUIRE(accumulator.has_value());
    CHECK(accumulator->strike() == 100.0);
    CHECK(accumulator->knock_out() == 110.0);
    CHECK(accumulator->daily_quantity() == 1.0);
    CHECK(accumulator->acceleration() == 2.0);
    CHECK(accumulator->accumulated_quantity() == 3.0);
    CHECK(accumulator->effective() == effective);
    CHECK(accumulator->expiry() == expiry);
}
