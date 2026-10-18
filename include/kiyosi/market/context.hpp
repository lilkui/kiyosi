#pragma once

#include <cmath>
#include <optional>
#include <utility>

#include <kiyosi/core/time.hpp>
#include <kiyosi/market/bsm_parameters.hpp>
#include <kiyosi/market/calendar.hpp>

namespace kiyosi {

class PricingContext;

/// Fields to replace; absent fields retain their current values, including the calendar.
struct PricingContextChanges {
    std::optional<BlackScholesMertonParameters> model_parameters;
    std::optional<double> spot_price;
    std::optional<Timestamp> valuation_time;
    std::optional<TradingCalendar> calendar;
};

[[nodiscard]] Result<PricingContext> make_pricing_context(
    BlackScholesMertonParameters, double, Timestamp, TradingCalendar);

/// Valuation-time snapshot: model parameters, the observable spot, and the trading calendar.
class PricingContext {
public:
    /// Returns a validated owning copy with the supplied fields replaced.
    /// Uses make_pricing_context validation and error categories; never changes this object.
    /// Concurrent calls and reads follow the calendar contract, without concurrent assignment.
    [[nodiscard]] Result<PricingContext> replace(const PricingContextChanges& changes = {}) const;

    /// Returns the Black-Scholes-Merton model parameters.
    const BlackScholesMertonParameters& model_parameters() const noexcept { return model_parameters_; }
    /// Returns the positive observable spot price.
    double spot_price() const noexcept { return spot_price_; }
    /// Returns the civil date containing the valuation time.
    Date valuation_date() const noexcept { return date_of(valuation_time_); }
    /// Returns the valuation timestamp.
    Timestamp valuation_time() const noexcept { return valuation_time_; }
    /// Returns the trading calendar.
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

/// Creates a pricing context at an intraday valuation time with an explicit calendar.
/// @return The context, or an `invalid_spot_price` or `invalid_date` error.
[[nodiscard]] inline Result<PricingContext> make_pricing_context(
    BlackScholesMertonParameters model_parameters, double spot_price, Timestamp valuation_time, TradingCalendar calendar)
{
    if (!std::isfinite(spot_price) || spot_price <= 0.0) {
        return std::unexpected(Error{ErrorCategory::invalid_spot_price,
                                     "spot price must be finite and positive"});
    }
    if (!is_supported_date(date_of(valuation_time))) {
        return std::unexpected(Error{ErrorCategory::invalid_date,
                                     "valuation time must contain a valid calendar date"});
    }
    return PricingContext{std::move(model_parameters), spot_price, valuation_time, std::move(calendar)};
}

/// Creates a pricing context at an intraday valuation time using weekdays_calendar().
/// @return The context, or an `invalid_spot_price` or `invalid_date` error.
[[nodiscard]] inline Result<PricingContext> make_pricing_context(
    BlackScholesMertonParameters model_parameters, double spot_price, Timestamp valuation_time)
{
    return make_pricing_context(std::move(model_parameters), spot_price, valuation_time, weekdays_calendar());
}

/// Creates a midnight pricing context for a date with an explicit calendar.
/// @return The context, or an `invalid_spot_price` or `invalid_date` error.
[[nodiscard]] inline Result<PricingContext> make_pricing_context(
    BlackScholesMertonParameters model_parameters, double spot_price, Date valuation_date, TradingCalendar calendar)
{
    if (!is_supported_date(valuation_date)) {
        return std::unexpected(Error{ErrorCategory::invalid_date,
                                     "valuation date must be a valid calendar date"});
    }
    return make_pricing_context(std::move(model_parameters), spot_price, start_of_day(valuation_date),
                                std::move(calendar));
}

/// Creates a midnight pricing context for a date using weekdays_calendar().
/// @return The context, or an `invalid_spot_price` or `invalid_date` error.
[[nodiscard]] inline Result<PricingContext> make_pricing_context(
    BlackScholesMertonParameters model_parameters, double spot_price, Date valuation_date)
{
    return make_pricing_context(std::move(model_parameters), spot_price, valuation_date, weekdays_calendar());
}

inline Result<PricingContext> PricingContext::replace(const PricingContextChanges& changes) const
{
    return make_pricing_context(changes.model_parameters.value_or(model_parameters_),
                                changes.spot_price.value_or(spot_price_),
                                changes.valuation_time.value_or(valuation_time_),
                                changes.calendar ? *changes.calendar : calendar_);
}

} // namespace kiyosi
