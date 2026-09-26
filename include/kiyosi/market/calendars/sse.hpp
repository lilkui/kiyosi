#pragma once

#include <kiyosi/market/calendar.hpp>
#include <kiyosi/kiyosi_export.h>

namespace kiyosi {

/// Inclusive years used to generate the bundled SSE holiday table.
inline constexpr int sse_calendar_data_first_year = 1901;
inline constexpr int sse_calendar_data_last_year = 2199;
/// Source version used to generate the bundled SSE holiday table.
inline constexpr const char* sse_calendar_data_version = "QuantLib 1.43";

/// Shanghai Stock Exchange: weekdays excluding the bundled QuantLib 1.43 holiday table.
/// The table spans 1901..2199, but future holidays may not yet be published by the exchange.
/// Outside that span, queries explicitly fall back to a holiday-unaware weekday calendar.
/// @return The SSE trading calendar with a 243-day annualization basis.
[[nodiscard]] KIYOSI_EXPORT TradingCalendar sse_calendar();

} // namespace kiyosi
