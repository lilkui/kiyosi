#include <kiyosi/core/day_count.hpp>

namespace kiyosi {

result<double> year_fraction(date start, date end, day_count_convention convention)
{
    if (!is_valid_date(start) || !is_valid_date(end))
        return std::unexpected(Error{error_category::invalid_date, "day-count dates must be valid calendar dates"});
    if (end < start)
        return std::unexpected(Error{error_category::invalid_expiry, "day-count end must not precede start"});
    if (convention != day_count_convention::actual_365_fixed)
        return std::unexpected(Error{error_category::invalid_parameter, "unsupported day-count convention"});
    return static_cast<double>((end - start).count()) / 365.0;
}

result<double> year_fraction(timestamp start, timestamp end, day_count_convention convention)
{
    if (!is_valid_date(date_of(start)) || !is_valid_date(date_of(end)))
        return std::unexpected(Error{error_category::invalid_date, "day-count timestamps must contain valid dates"});
    if (end < start)
        return std::unexpected(Error{error_category::invalid_expiry, "day-count end must not precede start"});
    if (convention != day_count_convention::actual_365_fixed)
        return std::unexpected(Error{error_category::invalid_parameter, "unsupported day-count convention"});
    return std::chrono::duration<double, std::ratio<86400 * 365>>{end - start}.count();
}

} // namespace kiyosi
