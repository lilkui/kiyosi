#pragma once

#include <kiyosi/market/calendar.hpp>
#include <kiyosi/kiyosi_export.h>

namespace kiyosi {

/// Shanghai Stock Exchange: weekdays excluding the published mainland holiday closures.
[[nodiscard]] KIYOSI_EXPORT TradingCalendar sse_calendar();

} // namespace kiyosi
