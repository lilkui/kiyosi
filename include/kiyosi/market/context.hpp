#pragma once

#include <cmath>
#include <utility>

#include <kiyosi/core/time.hpp>
#include <kiyosi/market/bsm_parameters.hpp>
#include <kiyosi/market/calendar.hpp>

namespace kiyosi {

class PricingContext;

[[nodiscard]] result<PricingContext> make_pricing_context(
    BsmParameters, double, timestamp, TradingCalendar);

/// Valuation-time snapshot: model parameters, the observable spot, and the trading calendar.
class PricingContext {
public:
    const BsmParameters& parameters() const noexcept { return parameters_; }
    double asset_price() const noexcept { return asset_price_; }
    date valuation_date() const noexcept { return date_of(valuation_time_); }
    timestamp valuation_time() const noexcept { return valuation_time_; }
    const TradingCalendar& calendar() const noexcept { return calendar_; }

private:
    PricingContext(BsmParameters parameters, double asset_price, timestamp valuation_time,
                   TradingCalendar calendar)
        : parameters_(std::move(parameters)), asset_price_(asset_price),
          valuation_time_(valuation_time), calendar_(std::move(calendar)) {}

    BsmParameters parameters_;
    double asset_price_;
    timestamp valuation_time_;
    TradingCalendar calendar_;

    friend result<PricingContext> make_pricing_context(
        BsmParameters, double, timestamp, TradingCalendar);
};

[[nodiscard]] inline result<PricingContext> make_pricing_context(
    BsmParameters parameters, double asset_price, timestamp valuation_time, TradingCalendar calendar)
{
    if (!std::isfinite(asset_price) || asset_price <= 0.0) {
        return std::unexpected(Error{error_category::invalid_asset_price,
                                     "asset price must be finite and positive"});
    }
    if (!is_valid_date(date_of(valuation_time))) {
        return std::unexpected(Error{error_category::invalid_date,
                                     "valuation time must contain a valid calendar date"});
    }
    return PricingContext{std::move(parameters), asset_price, valuation_time, std::move(calendar)};
}

[[nodiscard]] inline result<PricingContext> make_pricing_context(
    BsmParameters parameters, double asset_price, timestamp valuation_time)
{
    return make_pricing_context(std::move(parameters), asset_price, valuation_time, exchange_calendar());
}

[[nodiscard]] inline result<PricingContext> make_pricing_context(
    BsmParameters parameters, double asset_price, date valuation_date, TradingCalendar calendar)
{
    if (!is_valid_date(valuation_date)) {
        return std::unexpected(Error{error_category::invalid_date,
                                     "valuation date must be a valid calendar date"});
    }
    return make_pricing_context(std::move(parameters), asset_price, start_of_day(valuation_date),
                                std::move(calendar));
}

[[nodiscard]] inline result<PricingContext> make_pricing_context(
    BsmParameters parameters, double asset_price, date valuation_date)
{
    return make_pricing_context(std::move(parameters), asset_price, valuation_date, exchange_calendar());
}

} // namespace kiyosi
