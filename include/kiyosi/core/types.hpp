#pragma once

#include <chrono>
#include <cstdint>
#include <expected>
#include <string_view>
#include <kiyosi/kiyosi_export.h>

namespace kiyosi {

inline constexpr int version_major = 0;
inline constexpr int version_minor = 1;
inline constexpr int version_patch = 0;

using date = std::chrono::sys_days;
// Intraday moments use UTC-like sys_time; date-based contracts remain midnight anchored.
using timestamp = std::chrono::sys_time<std::chrono::nanoseconds>;
using time_point = timestamp;
[[nodiscard]] KIYOSI_EXPORT bool is_valid_date(date value) noexcept;

enum class day_count_convention : unsigned char {
    actual_365_fixed,
};

[[nodiscard]] inline timestamp start_of_day(date value) noexcept
{
    return timestamp{value.time_since_epoch()};
}

[[nodiscard]] inline date date_of(timestamp value) noexcept
{
    return date{std::chrono::floor<std::chrono::days>(value.time_since_epoch())};
}


enum class error_category : unsigned char {
    invalid_option = 1,
    invalid_strike = 2,
    invalid_volatility = 3,
    invalid_rate = 4,
    invalid_dividend = 5,
    invalid_asset_price = 6,
    invalid_date = 7,
    invalid_expiry = 8,
    invalid_result = 9,
    invalid_schedule = 10,
    invalid_calendar = 11,
    invalid_parameter = 12,
    invalid_quote = 15,
    unbracketed_volatility = 16,
    solver_non_convergence = 17,
    solver_non_finite = 18,
    unbracketed_coupon = 19,
};

struct Error {
    error_category category;
    std::string_view message;

    friend bool operator==(const Error&, const Error&) = default;
};

template <typename T>
using result = std::expected<T, Error>;

[[nodiscard]] KIYOSI_EXPORT result<double> year_fraction(
    date start, date end, day_count_convention convention = day_count_convention::actual_365_fixed);
[[nodiscard]] KIYOSI_EXPORT result<double> year_fraction(
    timestamp start, timestamp end, day_count_convention convention = day_count_convention::actual_365_fixed);
[[nodiscard]] KIYOSI_EXPORT result<void> validate_expiry(date valuation_date, date expiry);
[[nodiscard]] KIYOSI_EXPORT result<void> validate_expiry(timestamp valuation_time, date expiry);
} // namespace kiyosi
