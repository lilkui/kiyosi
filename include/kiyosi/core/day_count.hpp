#pragma once

#include <kiyosi/core/time.hpp>
#include <kiyosi/kiyosi_export.h>

namespace kiyosi {

namespace detail {

/// Computes Actual/365 Fixed for previously validated, ordered dates.
[[nodiscard]] inline double actual_365_fixed_year_fraction(Date start, Date end) noexcept
{
    return static_cast<double>((end - start).count()) / 365.0;
}

/// Computes Actual/365 Fixed for previously validated, ordered timestamps.
[[nodiscard]] inline double actual_365_fixed_year_fraction(Timestamp start, Timestamp end) noexcept
{
    return std::chrono::duration<double, std::ratio<31'536'000>>{end - start}.count();
}

} // namespace detail

/// Computes the Actual/365 Fixed year fraction between two dates.
/// @param start Inclusive start date.
/// @param end End date, which must not precede `start`.
/// @return Year fraction, or an `invalid_date` or `invalid_time_range` error.
[[nodiscard]] KIYOSI_EXPORT Result<double> year_fraction(
    Date start, Date end);
/// Computes the Actual/365 Fixed year fraction between two timestamps.
/// @param start Start timestamp.
/// @param end End timestamp, which must not precede `start`.
/// @return Year fraction, or an `invalid_date` or `invalid_time_range` error.
[[nodiscard]] KIYOSI_EXPORT Result<double> year_fraction(
    Timestamp start, Timestamp end);

} // namespace kiyosi
