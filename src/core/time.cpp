#include <kiyosi/core/time.hpp>

namespace kiyosi {

bool is_supported_date(Date value) noexcept
{
    constexpr auto first_supported = Date{std::chrono::year::min() / std::chrono::January / 1};
    constexpr auto last_supported = Date{std::chrono::year::max() / std::chrono::December / 31};
    if (value < first_supported || value > last_supported) return false;
    return std::chrono::year_month_day{value}.ok();
}

} // namespace kiyosi
