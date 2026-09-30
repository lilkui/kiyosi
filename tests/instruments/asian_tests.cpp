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
    const auto invalid_type = kiyosi::make_geometric_average_option(
        static_cast<kiyosi::OptionType>(99), 100.0, effective_date, averaging_start_date, expiry_date);
    REQUIRE_FALSE(invalid_type.has_value());
    CHECK(invalid_type.error().category == kiyosi::ErrorCategory::invalid_option);
    const auto invalid_strike = kiyosi::make_geometric_average_option(
        kiyosi::OptionType::call, 0.0, effective_date, averaging_start_date, expiry_date);
    REQUIRE_FALSE(invalid_strike.has_value());
    CHECK(invalid_strike.error().category == kiyosi::ErrorCategory::invalid_strike);
    const auto invalid_average = kiyosi::make_geometric_average_option(
        kiyosi::OptionType::call, 100.0, effective_date, averaging_start_date, expiry_date, -1.0);
    REQUIRE_FALSE(invalid_average.has_value());
    CHECK(invalid_average.error().category == kiyosi::ErrorCategory::invalid_parameter);
    const auto early_averaging = kiyosi::make_geometric_average_option(
        kiyosi::OptionType::call, 100.0, effective_date, effective_date - std::chrono::days{1}, expiry_date);
    REQUIRE_FALSE(early_averaging.has_value());
    CHECK(early_averaging.error().category == kiyosi::ErrorCategory::invalid_schedule);
    const auto late_averaging = kiyosi::make_geometric_average_option(
        kiyosi::OptionType::call, 100.0, effective_date, expiry_date + std::chrono::days{1}, expiry_date);
    REQUIRE_FALSE(late_averaging.has_value());
    CHECK(late_averaging.error().category == kiyosi::ErrorCategory::invalid_schedule);
}

TEST_CASE("Average option factories build distinct geometric and arithmetic variants")
{
    static_assert(!std::is_same_v<kiyosi::GeometricAveragePriceOption, kiyosi::ArithmeticAveragePriceOption>);

    const auto effective_date = day(2025, 1, 1);
    const auto expiry_date = day(2026, 1, 1);
    const auto averaging_start_date = day(2025, 7, 1);

    const auto geometric = kiyosi::make_geometric_average_option(
        kiyosi::OptionType::call, 100.0, effective_date, averaging_start_date, expiry_date, 95.0);
    REQUIRE(geometric.has_value());
    CHECK(geometric->option_type() == kiyosi::OptionType::call);
    CHECK(geometric->strike() == 100.0);
    CHECK(geometric->effective_date() == effective_date);
    CHECK(geometric->averaging_start_date() == averaging_start_date);
    CHECK(geometric->realized_average() == 95.0);
    CHECK(geometric->expiry_date() == expiry_date);

    const auto arithmetic = kiyosi::make_arithmetic_average_option(
        kiyosi::OptionType::put, 100.0, effective_date, averaging_start_date, expiry_date);
    REQUIRE(arithmetic.has_value());
    CHECK(arithmetic->option_type() == kiyosi::OptionType::put);
    CHECK(arithmetic->realized_average() == 0.0);
    CHECK(arithmetic->effective_date() == effective_date);
    CHECK(arithmetic->averaging_start_date() == averaging_start_date);
}
