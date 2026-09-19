#pragma once

#include <chrono>

#include <kiyosi/core/error.hpp>
#include <kiyosi/kiyosi_export.h>

namespace kiyosi {

using Date = std::chrono::sys_days;
// Intraday moments use UTC-like sys_time; Date-based contracts remain midnight anchored.
using Timestamp = std::chrono::sys_time<std::chrono::nanoseconds>;

/// Returns whether value is within the inclusive civil-Date range supported by std::chrono::year.
[[nodiscard]] KIYOSI_EXPORT bool is_valid_date(Date value) noexcept;

[[nodiscard]] inline Timestamp start_of_day(Date value) noexcept
{
    return Timestamp{value.time_since_epoch()};
}

[[nodiscard]] inline Date date_of(Timestamp value) noexcept
{
    return Date{std::chrono::floor<std::chrono::days>(value.time_since_epoch())};
}

[[nodiscard]] KIYOSI_EXPORT Result<void> validate_valuation_not_after_expiry(Date valuation_date, Date expiry_date);
[[nodiscard]] KIYOSI_EXPORT Result<void> validate_valuation_not_after_expiry(Timestamp valuation_time, Date expiry_date);

[[nodiscard]] inline Result<void> validate_valuation_within_instrument_life(Date valuation_date, Date effective_date, Date expiry_date)
{
    if (!is_valid_date(valuation_date) || !is_valid_date(effective_date) || !is_valid_date(expiry_date))
        return std::unexpected(Error{ErrorCategory::invalid_date, "life dates must be valid calendar dates"});
    if (effective_date > expiry_date)
        return std::unexpected(Error{ErrorCategory::invalid_expiry, "effective date must not follow expiry_date"});
    if (valuation_date < effective_date || valuation_date > expiry_date)
        return std::unexpected(Error{ErrorCategory::invalid_expiry, "valuation date must be within the instrument life"});
    return {};
}

[[nodiscard]] inline Result<void> validate_valuation_within_instrument_life(Timestamp valuation_time, Date effective_date, Date expiry_date)
{
    auto valid = validate_valuation_within_instrument_life(date_of(valuation_time), effective_date, expiry_date);
    if (!valid) return valid;
    return validate_valuation_not_after_expiry(valuation_time, expiry_date);
}

} // namespace kiyosi
