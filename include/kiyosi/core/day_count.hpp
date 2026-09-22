#pragma once

#include <kiyosi/core/time.hpp>
#include <kiyosi/kiyosi_export.h>

namespace kiyosi {

/// Supported day-count conventions.
enum class DayCountConvention : unsigned char {
    actual_365_fixed, ///< Actual elapsed time divided by 365 days.
};

/// Computes the year fraction between two dates.
/// @param start Inclusive start date.
/// @param end End date, which must not precede `start`.
/// @param convention Day-count convention to apply.
/// @return Year fraction, or an `invalid_date`, `invalid_time_range`, or `invalid_parameter` error.
[[nodiscard]] KIYOSI_EXPORT Result<double> year_fraction(
    Date start, Date end, DayCountConvention convention = DayCountConvention::actual_365_fixed);
/// Computes the year fraction between two timestamps.
/// @param start Start timestamp.
/// @param end End timestamp, which must not precede `start`.
/// @param convention Day-count convention to apply.
/// @return Year fraction, or an `invalid_date`, `invalid_time_range`, or `invalid_parameter` error.
[[nodiscard]] KIYOSI_EXPORT Result<double> year_fraction(
    Timestamp start, Timestamp end, DayCountConvention convention = DayCountConvention::actual_365_fixed);

} // namespace kiyosi
