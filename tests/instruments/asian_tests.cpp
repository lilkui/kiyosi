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
    const auto effective_date = day(2025, 1, 1);
    const auto expiry_date = day(2026, 1, 1);
    const auto averaging_start_date = day(2025, 7, 1);
    CHECK(kiyosi::make_geometric_average_option(
              static_cast<kiyosi::OptionType>(99), 100.0, averaging_start_date, effective_date, expiry_date)
              .error()
              .category == kiyosi::ErrorCategory::invalid_option);
    CHECK(kiyosi::make_geometric_average_option(
              kiyosi::OptionType::call, 0.0, averaging_start_date, effective_date, expiry_date)
              .error()
              .category == kiyosi::ErrorCategory::invalid_strike);
    CHECK(kiyosi::make_geometric_average_option(
              kiyosi::OptionType::call, 100.0, averaging_start_date, effective_date, expiry_date, -1.0)
              .error()
              .category == kiyosi::ErrorCategory::invalid_parameter);
    CHECK(kiyosi::make_geometric_average_option(
              kiyosi::OptionType::call, 100.0, effective_date - std::chrono::days{1}, effective_date, expiry_date)
              .error()
              .category == kiyosi::ErrorCategory::invalid_schedule);
    CHECK(kiyosi::make_geometric_average_option(
              kiyosi::OptionType::call, 100.0, expiry_date + std::chrono::days{1}, effective_date, expiry_date)
              .error()
              .category == kiyosi::ErrorCategory::invalid_schedule);
}

TEST_CASE("Average option factories build distinct geometric and arithmetic variants")
{
    static_assert(!std::is_same_v<kiyosi::GeometricAveragePriceOption, kiyosi::ArithmeticAveragePriceOption>);

    const auto effective_date = day(2025, 1, 1);
    const auto expiry_date = day(2026, 1, 1);
    const auto averaging_start_date = day(2025, 7, 1);

    const auto geometric = kiyosi::make_geometric_average_option(
        kiyosi::OptionType::call, 100.0, averaging_start_date, effective_date, expiry_date, 95.0);
    REQUIRE(geometric.has_value());
    CHECK(geometric->option_type() == kiyosi::OptionType::call);
    CHECK(geometric->strike() == 100.0);
    CHECK(geometric->averaging_start_date() == averaging_start_date);
    CHECK(geometric->realized_average() == 95.0);
    CHECK(geometric->expiry_date() == expiry_date);

    const auto arithmetic = kiyosi::make_arithmetic_average_option(
        kiyosi::OptionType::put, 100.0, averaging_start_date, effective_date, expiry_date);
    REQUIRE(arithmetic.has_value());
    CHECK(arithmetic->option_type() == kiyosi::OptionType::put);
    CHECK(arithmetic->realized_average() == 0.0);
}
