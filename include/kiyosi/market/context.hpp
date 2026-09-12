#pragma once

#include <cmath>
#include <utility>
#include <kiyosi/core/types.hpp>
#include <kiyosi/market/calendar.hpp>

namespace kiyosi {

class BsmParameters {
public:
    double risk_free_rate() const noexcept { return risk_free_rate_; }
    double dividend_yield() const noexcept { return dividend_yield_; }
    double volatility() const noexcept { return volatility_; }

    friend bool operator==(const BsmParameters&, const BsmParameters&) = default;

private:
    BsmParameters(double risk_free_rate, double dividend_yield, double volatility)
        : risk_free_rate_(risk_free_rate), dividend_yield_(dividend_yield), volatility_(volatility) {}

    double risk_free_rate_;
    double dividend_yield_;
    double volatility_;

    friend result<BsmParameters> make_bsm_parameters(double, double, double);
};

[[nodiscard]] inline result<BsmParameters> make_bsm_parameters(
    double risk_free_rate, double dividend_yield, double volatility)
{
    if (!std::isfinite(risk_free_rate))
        return std::unexpected(Error{error_category::invalid_rate, "risk-free rate must be finite"});
    if (!std::isfinite(dividend_yield))
        return std::unexpected(Error{error_category::invalid_dividend, "dividend yield must be finite"});
    if (!std::isfinite(volatility) || volatility <= 0.0) {
        return std::unexpected(Error{error_category::invalid_volatility,
                                     "volatility must be finite and positive"});
    }
    return BsmParameters{risk_free_rate, dividend_yield, volatility};
}

class AssetPrice {
public:
    double value() const noexcept { return value_; }

    friend bool operator==(const AssetPrice&, const AssetPrice&) = default;

private:
    explicit AssetPrice(double value) : value_(value) {}
    double value_;
    friend result<AssetPrice> make_asset_price(double);
};

class MarketState {
public:
    const BsmParameters& parameters() const noexcept { return parameters_; }
    AssetPrice asset_price() const noexcept { return asset_price_; }
    date valuation_date() const noexcept { return date_of(valuation_time_); }
    timestamp valuation_time() const noexcept { return valuation_time_; }

private:
    MarketState(BsmParameters parameters, AssetPrice asset_price, timestamp valuation_time)
        : parameters_(std::move(parameters)), asset_price_(asset_price), valuation_time_(valuation_time) {}

    BsmParameters parameters_;
    AssetPrice asset_price_;
    timestamp valuation_time_;

    friend class PricingContext;
};

[[nodiscard]] inline result<AssetPrice> make_asset_price(double value)
{
    if (!std::isfinite(value) || value <= 0.0) {
        return std::unexpected(Error{error_category::invalid_asset_price,
                                     "asset price must be finite and positive"});
    }
    return AssetPrice{value};
}

class PricingContext {
public:
    const BsmParameters& parameters() const noexcept { return market_.parameters(); }
    AssetPrice asset_price() const noexcept { return market_.asset_price(); }
    date valuation_date() const noexcept { return market_.valuation_date(); }
    timestamp valuation_time() const noexcept { return market_.valuation_time(); }
    const MarketState& market() const noexcept { return market_; }
    const TradingCalendar& calendar() const noexcept { return calendar_; }

private:
    PricingContext(BsmParameters parameters, AssetPrice asset_price, timestamp valuation_time,
                   TradingCalendar calendar)
        : market_(std::move(parameters), asset_price, valuation_time),
          calendar_(std::move(calendar)) {}

    MarketState market_;
    TradingCalendar calendar_;

    friend result<PricingContext> make_pricing_context(BsmParameters, AssetPrice, date, TradingCalendar);
    friend result<PricingContext> make_pricing_context(BsmParameters, AssetPrice, timestamp, TradingCalendar);
};

[[nodiscard]] inline result<PricingContext> make_pricing_context(
    BsmParameters, AssetPrice, date, TradingCalendar);

[[nodiscard]] inline result<PricingContext> make_pricing_context(
    BsmParameters parameters, AssetPrice asset_price, timestamp valuation_time, TradingCalendar calendar)
{
    if (!is_valid_date(date_of(valuation_time)))
        return std::unexpected(Error{error_category::invalid_date,
                                     "valuation time must contain a valid calendar date"});
    return PricingContext{std::move(parameters), asset_price, valuation_time, std::move(calendar)};
}

[[nodiscard]] inline result<PricingContext> make_pricing_context(
    BsmParameters parameters, AssetPrice asset_price, timestamp valuation_time)
{
    return make_pricing_context(std::move(parameters), asset_price, valuation_time, all_days_calendar());
}

[[nodiscard]] inline result<PricingContext> make_pricing_context(
    BsmParameters parameters, AssetPrice asset_price, date valuation_date)
{
    return make_pricing_context(std::move(parameters), asset_price, start_of_day(valuation_date), all_days_calendar());
}

[[nodiscard]] inline result<PricingContext> make_pricing_context(
    BsmParameters parameters, AssetPrice asset_price, date valuation_date, TradingCalendar calendar)
{
    if (!is_valid_date(valuation_date)) {
        return std::unexpected(Error{error_category::invalid_date,
                                     "valuation date must be a valid calendar date"});
    }
    return PricingContext{std::move(parameters), asset_price, start_of_day(valuation_date), std::move(calendar)};
}

} // namespace kiyosi
