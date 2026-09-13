#include <catch2/catch_test_macros.hpp>

#include <chrono>
#include <type_traits>

#include <kiyosi/kiyosi.hpp>

#include "support/common.hpp"

namespace {
using kiyosi::test::day;
}

TEST_CASE("Asian option terms reject invalid averaging windows")
{
    const auto effective = day(2025, 1, 1);
    const auto expiry = day(2026, 1, 1);
    const auto average_start = day(2025, 7, 1);
    CHECK(kiyosi::make_geometric_average_option(
              static_cast<kiyosi::option_type>(99), 100.0, average_start, effective, expiry)
              .error()
              .category == kiyosi::error_category::invalid_option);
    CHECK(kiyosi::make_geometric_average_option(
              kiyosi::option_type::call, 0.0, average_start, effective, expiry)
              .error()
              .category == kiyosi::error_category::invalid_strike);
    CHECK(kiyosi::make_geometric_average_option(
              kiyosi::option_type::call, 100.0, average_start, effective, expiry, -1.0)
              .error()
              .category == kiyosi::error_category::invalid_parameter);
    CHECK(kiyosi::make_geometric_average_option(
              kiyosi::option_type::call, 100.0, effective - std::chrono::days{1}, effective, expiry)
              .error()
              .category == kiyosi::error_category::invalid_schedule);
    CHECK(kiyosi::make_geometric_average_option(
              kiyosi::option_type::call, 100.0, expiry + std::chrono::days{1}, effective, expiry)
              .error()
              .category == kiyosi::error_category::invalid_schedule);
}

TEST_CASE("Average option factories build distinct geometric and arithmetic variants")
{
    static_assert(!std::is_same_v<kiyosi::GeometricAverageOption, kiyosi::ArithmeticAverageOption>);

    const auto effective = day(2025, 1, 1);
    const auto expiry = day(2026, 1, 1);
    const auto average_start = day(2025, 7, 1);

    const auto geometric = kiyosi::make_geometric_average_option(
        kiyosi::option_type::call, 100.0, average_start, effective, expiry, 95.0);
    REQUIRE(geometric.has_value());
    CHECK(geometric->type() == kiyosi::option_type::call);
    CHECK(geometric->strike() == 100.0);
    CHECK(geometric->average_start() == average_start);
    CHECK(geometric->realized_average() == 95.0);
    CHECK(geometric->expiry() == expiry);

    const auto arithmetic = kiyosi::make_arithmetic_average_option(
        kiyosi::option_type::put, 100.0, average_start, effective, expiry);
    REQUIRE(arithmetic.has_value());
    CHECK(arithmetic->type() == kiyosi::option_type::put);
    CHECK(arithmetic->realized_average() == 0.0);
}
