#pragma once

#include <chrono>

#include <kiyosi/core/error.hpp>
#include <kiyosi/kiyosi_export.h>

namespace kiyosi {

/// Civil date represented as a day on the system clock timeline.
using Date = std::chrono::sys_days;
/// UTC-like instant with microsecond precision; date-only contracts are midnight anchored.
/// Its range covers every supported civil date and the elapsed time between any two of them.
using Timestamp = std::chrono::sys_time<std::chrono::microseconds>;

/// Tests whether a date is representable by `std::chrono::year`.
/// @param value Date to test.
/// @return `true` when `value` is in the supported inclusive civil-date range.
[[nodiscard]] KIYOSI_EXPORT bool is_supported_date(Date value) noexcept;

/// Converts a date to its midnight timestamp.
/// @param value Date to convert.
/// @return Timestamp at the start of `value`.
[[nodiscard]] inline Timestamp start_of_day(Date value) noexcept
{
    return Timestamp{value.time_since_epoch()};
}

/// Extracts the civil date containing a timestamp.
/// @param value Timestamp to convert.
/// @return Date obtained by flooring `value` to whole days.
[[nodiscard]] inline Date date_of(Timestamp value) noexcept
{
    return Date{std::chrono::floor<std::chrono::days>(value.time_since_epoch())};
}

/// Validates that a date valuation does not follow an expiry date.
/// @return Success, or an `invalid_date` or `invalid_time_range` error.
[[nodiscard]] KIYOSI_EXPORT Result<void> validate_valuation_not_after_expiry(Date valuation_date, Date expiry_date);
/// Validates that a timestamp valuation does not follow the end of an expiry date.
/// @return Success, or an `invalid_date` or `invalid_time_range` error.
[[nodiscard]] KIYOSI_EXPORT Result<void> validate_valuation_not_after_expiry(Timestamp valuation_time, Date expiry_date);

/// Validates the inclusive dates of an instrument's life.
/// @return Success, `invalid_date` for unsupported dates, or `invalid_time_range` for reversed dates.
[[nodiscard]] inline Result<void> validate_instrument_life(Date effective_date, Date expiry_date)
{
    if (!is_supported_date(effective_date) || !is_supported_date(expiry_date))
        return std::unexpected(Error{ErrorCategory::invalid_date, "life dates must be valid calendar dates"});
    if (effective_date > expiry_date)
        return std::unexpected(Error{ErrorCategory::invalid_time_range,
                                     "expiry date must not precede the effective date"});
    return {};
}

/// Validates that a valuation date lies within an instrument's inclusive life.
/// @return Success, or an `invalid_date` or `invalid_time_range` error.
[[nodiscard]] inline Result<void> validate_valuation_within_instrument_life(Date valuation_date, Date effective_date, Date expiry_date)
{
    if (!is_supported_date(valuation_date))
        return std::unexpected(Error{ErrorCategory::invalid_date, "valuation date must be a valid calendar date"});
    auto life = validate_instrument_life(effective_date, expiry_date);
    if (!life) return life;
    if (valuation_date < effective_date || valuation_date > expiry_date)
        return std::unexpected(Error{ErrorCategory::invalid_time_range, "valuation date must be within the instrument life"});
    return {};
}

/// Validates that a valuation timestamp lies within an instrument's inclusive life.
/// @return Success, or an `invalid_date` or `invalid_time_range` error.
[[nodiscard]] inline Result<void> validate_valuation_within_instrument_life(Timestamp valuation_time, Date effective_date, Date expiry_date)
{
    auto valid = validate_valuation_within_instrument_life(date_of(valuation_time), effective_date, expiry_date);
    if (!valid) return valid;
    return validate_valuation_not_after_expiry(valuation_time, expiry_date);
}

} // namespace kiyosi
