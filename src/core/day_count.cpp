#include <kiyosi/core/day_count.hpp>

namespace kiyosi {

Result<double> year_fraction(Date start, Date end)
{
    if (!is_supported_date(start) || !is_supported_date(end))
        return std::unexpected(Error{ErrorCategory::invalid_date, "day-count dates must be valid calendar dates"});
    if (end < start)
        return std::unexpected(Error{ErrorCategory::invalid_time_range, "day-count end must not precede start"});
    return detail::actual_365_fixed_year_fraction(start, end);
}

Result<double> year_fraction(Timestamp start, Timestamp end)
{
    if (!is_supported_date(date_of(start)) || !is_supported_date(date_of(end)))
        return std::unexpected(Error{ErrorCategory::invalid_date, "day-count timestamps must contain valid dates"});
    if (end < start)
        return std::unexpected(Error{ErrorCategory::invalid_time_range, "day-count end must not precede start"});
    return detail::actual_365_fixed_year_fraction(start, end);
}

} // namespace kiyosi
