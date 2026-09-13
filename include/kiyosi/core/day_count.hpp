#pragma once

#include <kiyosi/core/time.hpp>
#include <kiyosi/kiyosi_export.h>

namespace kiyosi {

enum class day_count_convention : unsigned char {
    actual_365_fixed,
};

[[nodiscard]] KIYOSI_EXPORT result<double> year_fraction(
    date start, date end, day_count_convention convention = day_count_convention::actual_365_fixed);
[[nodiscard]] KIYOSI_EXPORT result<double> year_fraction(
    timestamp start, timestamp end, day_count_convention convention = day_count_convention::actual_365_fixed);

} // namespace kiyosi
