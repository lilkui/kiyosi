#pragma once

#include <cmath>
#include <utility>

#include <kiyosi/core/time.hpp>
#include <kiyosi/market/bsm_parameters.hpp>
#include <kiyosi/market/calendar.hpp>

namespace kiyosi {

class PricingContext;

[[nodiscard]] Result<PricingContext> make_pricing_context(
    BlackScholesMertonParameters, double, Timestamp, TradingCalendar);

/// Valuation-time snapshot: model parameters, the observable spot, and the trading calendar.
class PricingContext {
public:
    const BlackScholesMertonParameters& model_parameters() const noexcept { return model_parameters_; }
    double spot_price() const noexcept { return spot_price_; }
    Date valuation_date() const noexcept { return date_of(valuation_time_); }
    Timestamp valuation_time() const noexcept { return valuation_time_; }
    const TradingCalendar& calendar() const noexcept { return calendar_; }

private:
    PricingContext(BlackScholesMertonParameters model_parameters, double spot_price, Timestamp valuation_time,
                   TradingCalendar calendar)
        : model_parameters_(std::move(model_parameters)), spot_price_(spot_price),
          valuation_time_(valuation_time), calendar_(std::move(calendar)) {}

    BlackScholesMertonParameters model_parameters_;
    double spot_price_;
    Timestamp valuation_time_;
    TradingCalendar calendar_;

    friend Result<PricingContext> make_pricing_context(
        BlackScholesMertonParameters, double, Timestamp, TradingCalendar);
};

[[nodiscard]] inline Result<PricingContext> make_pricing_context(
    BlackScholesMertonParameters model_parameters, double spot_price, Timestamp valuation_time, TradingCalendar calendar)
{
    if (!std::isfinite(spot_price) || spot_price <= 0.0) {
        return std::unexpected(Error{ErrorCategory::invalid_spot_price,
                                     "spot price must be finite and positive"});
    }
    if (!is_valid_date(date_of(valuation_time))) {
        return std::unexpected(Error{ErrorCategory::invalid_date,
                                     "valuation time must contain a valid calendar date"});
    }
    return PricingContext{std::move(model_parameters), spot_price, valuation_time, std::move(calendar)};
}

[[nodiscard]] inline Result<PricingContext> make_pricing_context(
    BlackScholesMertonParameters model_parameters, double spot_price, Timestamp valuation_time)
{
    return make_pricing_context(std::move(model_parameters), spot_price, valuation_time, weekdays_calendar());
}

[[nodiscard]] inline Result<PricingContext> make_pricing_context(
    BlackScholesMertonParameters model_parameters, double spot_price, Date valuation_date, TradingCalendar calendar)
{
    if (!is_valid_date(valuation_date)) {
        return std::unexpected(Error{ErrorCategory::invalid_date,
                                     "valuation date must be a valid calendar date"});
    }
    return make_pricing_context(std::move(model_parameters), spot_price, start_of_day(valuation_date),
                                std::move(calendar));
}

[[nodiscard]] inline Result<PricingContext> make_pricing_context(
    BlackScholesMertonParameters model_parameters, double spot_price, Date valuation_date)
{
    return make_pricing_context(std::move(model_parameters), spot_price, valuation_date, weekdays_calendar());
}

} // namespace kiyosi
