#include <kiyosi/core/day_count.hpp>

namespace kiyosi {

Result<double> year_fraction(Date start, Date end, DayCountConvention convention)
{
    if (!is_supported_date(start) || !is_supported_date(end))
        return std::unexpected(Error{ErrorCategory::invalid_date, "day-count dates must be valid calendar dates"});
    if (end < start)
        return std::unexpected(Error{ErrorCategory::invalid_time_range, "day-count end must not precede start"});
    if (convention != DayCountConvention::actual_365_fixed)
        return std::unexpected(Error{ErrorCategory::invalid_parameter, "unsupported day-count convention"});
    return detail::actual_365_fixed_year_fraction(start, end);
}

Result<double> year_fraction(Timestamp start, Timestamp end, DayCountConvention convention)
{
    if (!is_supported_date(date_of(start)) || !is_supported_date(date_of(end)))
        return std::unexpected(Error{ErrorCategory::invalid_date, "day-count timestamps must contain valid dates"});
    if (end < start)
        return std::unexpected(Error{ErrorCategory::invalid_time_range, "day-count end must not precede start"});
    if (convention != DayCountConvention::actual_365_fixed)
        return std::unexpected(Error{ErrorCategory::invalid_parameter, "unsupported day-count convention"});
    return detail::actual_365_fixed_year_fraction(start, end);
}

} // namespace kiyosi
