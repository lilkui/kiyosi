#pragma once

#include <kiyosi/market/calendar.hpp>
#include <kiyosi/kiyosi_export.h>

namespace kiyosi {

/// Shanghai Stock Exchange: weekdays excluding recorded XSHG and forecast holidays.
/// The table spans 1991..2099; dates beyond the upstream list are estimates.
/// Outside that span, queries fall back to a holiday-unaware weekday calendar.
/// @return The SSE trading calendar with a 243-day annualization basis.
[[nodiscard]] KIYOSI_EXPORT TradingCalendar sse_calendar();

} // namespace kiyosi
