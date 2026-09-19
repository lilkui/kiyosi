#pragma once

#include <chrono>

#include <kiyosi/core/error.hpp>
#include <kiyosi/kiyosi_export.h>

namespace kiyosi {

using date = std::chrono::sys_days;
// Intraday moments use UTC-like sys_time; date-based contracts remain midnight anchored.
using timestamp = std::chrono::sys_time<std::chrono::nanoseconds>;

/// Returns whether value is within the inclusive civil-date range supported by std::chrono::year.
[[nodiscard]] KIYOSI_EXPORT bool is_valid_date(date value) noexcept;

[[nodiscard]] inline timestamp start_of_day(date value) noexcept
{
    return timestamp{value.time_since_epoch()};
}

[[nodiscard]] inline date date_of(timestamp value) noexcept
{
    return date{std::chrono::floor<std::chrono::days>(value.time_since_epoch())};
}

[[nodiscard]] KIYOSI_EXPORT result<void> validate_expiry(date valuation_date, date expiry);
[[nodiscard]] KIYOSI_EXPORT result<void> validate_expiry(timestamp valuation_time, date expiry);

[[nodiscard]] inline result<void> validate_life(date valuation_date, date effective, date expiry)
{
    if (!is_valid_date(valuation_date) || !is_valid_date(effective) || !is_valid_date(expiry))
        return std::unexpected(Error{error_category::invalid_date, "life dates must be valid calendar dates"});
    if (effective > expiry)
        return std::unexpected(Error{error_category::invalid_expiry, "effective date must not follow expiry"});
    if (valuation_date < effective || valuation_date > expiry)
        return std::unexpected(Error{error_category::invalid_expiry, "valuation date must be within the instrument life"});
    return {};
}

[[nodiscard]] inline result<void> validate_life(timestamp valuation_time, date effective, date expiry)
{
    auto valid = validate_life(date_of(valuation_time), effective, expiry);
    if (!valid) return valid;
    return validate_expiry(valuation_time, expiry);
}

} // namespace kiyosi
