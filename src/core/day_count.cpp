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
    return static_cast<double>((end - start).count()) / 365.0;
}

Result<double> year_fraction(Timestamp start, Timestamp end, DayCountConvention convention)
{
    if (!is_supported_date(date_of(start)) || !is_supported_date(date_of(end)))
        return std::unexpected(Error{ErrorCategory::invalid_date, "day-count timestamps must contain valid dates"});
    if (end < start)
        return std::unexpected(Error{ErrorCategory::invalid_time_range, "day-count end must not precede start"});
    if (convention != DayCountConvention::actual_365_fixed)
        return std::unexpected(Error{ErrorCategory::invalid_parameter, "unsupported day-count convention"});
    return std::chrono::duration<double, std::ratio<31'536'000>>{end - start}.count();
}

} // namespace kiyosi
