#pragma once

#include <kiyosi/core/time.hpp>
#include <kiyosi/kiyosi_export.h>

namespace kiyosi {

enum class DayCountConvention : unsigned char {
    actual_365_fixed,
};

[[nodiscard]] KIYOSI_EXPORT Result<double> year_fraction(
    Date start, Date end, DayCountConvention convention = DayCountConvention::actual_365_fixed);
[[nodiscard]] KIYOSI_EXPORT Result<double> year_fraction(
    Timestamp start, Timestamp end, DayCountConvention convention = DayCountConvention::actual_365_fixed);

} // namespace kiyosi
