#pragma once

#include <chrono>
#include <cmath>
#include <expected>
#include <functional>
#include <span>
#include <string>
#include <utility>
#include <vector>

namespace ito {

inline constexpr int version_major = 0;
inline constexpr int version_minor = 1;
inline constexpr int version_patch = 0;

using date = std::chrono::sys_days;
using Date = date;

inline bool is_valid_date(date value) noexcept;

enum class error_category : unsigned char {
    invalid_option = 1,
    invalid_parameter = 2,
    invalid_asset_price = 3,
    invalid_date = 4,
    invalid_expiry = 5,
    invalid_result = 6,
    invalid_schedule = 7,
    invalid_calendar = 8,
    invalid_strike = invalid_option,
    invalid_volatility = invalid_parameter,
    invalid_rate = invalid_parameter,
    invalid_dividend = invalid_parameter,
    InvalidOption = invalid_option,
    InvalidParameter = invalid_parameter,
    InvalidAssetPrice = invalid_asset_price,
    InvalidDate = invalid_date,
    InvalidExpiry = invalid_expiry,
    InvalidResult = invalid_result,
    InvalidSchedule = invalid_schedule,
    InvalidCalendar = invalid_calendar,
};

using ErrorCategory = error_category;

struct Error {
    const error_category category;
    const std::string message;

    friend bool operator==(const Error&, const Error&) = default;
};

using error = Error;

template <typename T>
using result = std::expected<T, Error>;

enum class option_type {
    call,
    put,
    Call = call,
    Put = put,
};

using OptionType = option_type;

class EuropeanOption {
public:
    option_type type() const noexcept { return type_; }
    double strike() const noexcept { return strike_; }
    date expiry() const noexcept { return expiry_; }
    date expiration() const noexcept { return expiry_; }

    friend bool operator==(const EuropeanOption&, const EuropeanOption&) = default;

private:
    EuropeanOption(option_type type, double strike, date expiry)
        : type_(type), strike_(strike), expiry_(expiry) {}

    option_type type_;
    double strike_;
    date expiry_;

    friend result<EuropeanOption> make_european_option(option_type, double, date);
};

using european_option = EuropeanOption;
using EuropeanOptionTerms = EuropeanOption;

inline result<EuropeanOption> make_european_option(option_type type, double strike, date expiry)
{
    if (type != option_type::call && type != option_type::put) {
        return std::unexpected(Error{error_category::invalid_option, "option type must be call or put"});
    }
    if (!std::isfinite(strike) || strike <= 0.0) {
        return std::unexpected(Error{error_category::invalid_option, "strike must be finite and positive"});
    }
    if (!is_valid_date(expiry)) {
        return std::unexpected(Error{error_category::invalid_date, "expiry must be a valid calendar date"});
    }
    return EuropeanOption{type, strike, expiry};
}

inline result<EuropeanOption> make_european_option(double strike, date expiry, option_type type)
{
    return make_european_option(type, strike, expiry);
}

inline result<void> validate_expiry(date valuation_date, date expiry);

inline bool is_valid_date(date value) noexcept
{
    return std::chrono::year_month_day{value}.ok();
}

class TradingCalendar {
public:
    using trading_day_predicate = std::function<bool(date)>;

    TradingCalendar(trading_day_predicate predicate, int annual_trading_days)
        : predicate_(std::move(predicate)), annual_trading_days_(annual_trading_days) {}

    bool is_trading_day(date value) const
    {
        return is_valid_date(value) && predicate_ && predicate_(value);
    }

    bool is_trading_date(date value) const { return is_trading_day(value); }
    bool valid(date value) const { return is_trading_day(value); }
    bool contains(date value) const { return is_trading_day(value); }
    bool operator()(date value) const { return is_trading_day(value); }
    int annual_trading_days() const noexcept { return annual_trading_days_; }
    int annual_trading_day_count() const noexcept { return annual_trading_days_; }
    int annual_count() const noexcept { return annual_trading_days_; }
    int trading_days() const noexcept { return annual_trading_days_; }
    int trading_days_per_year() const noexcept { return annual_trading_days_; }

    friend bool operator==(const TradingCalendar& left, const TradingCalendar& right) noexcept
    {
        return left.annual_trading_days_ == right.annual_trading_days_ &&
               left.predicate_.target_type() == right.predicate_.target_type();
    }

private:
    trading_day_predicate predicate_;
    int annual_trading_days_;
};

using trading_calendar = TradingCalendar;
using Calendar = TradingCalendar;

inline result<TradingCalendar> make_trading_calendar(
    TradingCalendar::trading_day_predicate predicate, int annual_trading_days)
{
    if (!predicate) {
        return std::unexpected(Error{error_category::invalid_calendar,
                                     "trading calendar requires a day predicate"});
    }
    if (annual_trading_days <= 0) {
        return std::unexpected(Error{error_category::invalid_calendar,
                                     "annual trading-day count must be positive"});
    }
    return TradingCalendar{std::move(predicate), annual_trading_days};
}

inline result<TradingCalendar> make_custom_trading_calendar(
    TradingCalendar::trading_day_predicate predicate, int annual_trading_days)
{
    return make_trading_calendar(std::move(predicate), annual_trading_days);
}

inline TradingCalendar all_days_calendar()
{
    return TradingCalendar{[](date) { return true; }, 365};
}

inline TradingCalendar all_days() { return all_days_calendar(); }
inline TradingCalendar make_all_days_calendar() { return all_days_calendar(); }

inline TradingCalendar exchange_calendar()
{
    return TradingCalendar{[](date value) {
        const auto weekday = std::chrono::weekday{value};
        return weekday != std::chrono::Saturday && weekday != std::chrono::Sunday;
    }, 252};
}

inline TradingCalendar exchange_style_calendar() { return exchange_calendar(); }
inline TradingCalendar exchange_style() { return exchange_calendar(); }
inline TradingCalendar make_exchange_calendar() { return exchange_calendar(); }
inline TradingCalendar make_all_days_trading_calendar() { return all_days_calendar(); }
inline TradingCalendar make_exchange_style_calendar() { return exchange_calendar(); }

inline result<void> validate_observation_date(
    date observation, date instrument_start, date instrument_end, const TradingCalendar& calendar)
{
    if (!is_valid_date(observation) || !is_valid_date(instrument_start) || !is_valid_date(instrument_end) ||
        instrument_end < instrument_start || observation < instrument_start || instrument_end < observation) {
        return std::unexpected(Error{error_category::invalid_date,
                                     "observation date must be within the instrument life"});
    }
    if (!calendar.is_trading_day(observation)) {
        return std::unexpected(Error{error_category::invalid_date,
                                     "observation date is not a trading day"});
    }
    return {};
}

inline result<void> validate_observation_dates(
    std::span<const date> observations, date instrument_start, date instrument_end,
    const TradingCalendar& calendar)
{
    if (!is_valid_date(instrument_start) || !is_valid_date(instrument_end) || instrument_end < instrument_start) {
        return std::unexpected(Error{error_category::invalid_date,
                                     "instrument life must be a valid ordered date range"});
    }
    for (std::size_t index = 0; index < observations.size(); ++index) {
        if (index > 0 && observations[index] < observations[index - 1]) {
            return std::unexpected(Error{error_category::invalid_date,
                                         "observation dates must be ordered"});
        }
        auto valid = validate_observation_date(observations[index], instrument_start, instrument_end, calendar);
        if (!valid) return std::unexpected(valid.error());
    }
    return {};
}

inline result<void> validate_schedule(
    std::span<const date> observations, date instrument_start, date instrument_end,
    const TradingCalendar& calendar)
{
    return validate_observation_dates(observations, instrument_start, instrument_end, calendar);
}

inline result<void> validate_schedule(
    const std::vector<date>& observations, date instrument_start, date instrument_end,
    const TradingCalendar& calendar)
{
    return validate_observation_dates(observations, instrument_start, instrument_end, calendar);
}

inline result<void> validate_observation_dates(
    std::span<const date> observations, date valuation_date, const EuropeanOption& option,
    const TradingCalendar& calendar)
{
    return validate_observation_dates(observations, valuation_date, option.expiry(), calendar);
}

inline result<void> validate_schedule(
    std::span<const date> observations, date valuation_date, const EuropeanOption& option,
    const TradingCalendar& calendar)
{
    return validate_observation_dates(observations, valuation_date, option, calendar);
}

class ObservationSchedule {
public:
    const std::vector<date>& dates() const noexcept { return dates_; }
    const std::vector<date>& observation_dates() const noexcept { return dates_; }
    std::size_t size() const noexcept { return dates_.size(); }
    bool empty() const noexcept { return dates_.empty(); }
    const date& operator[](std::size_t index) const noexcept { return dates_[index]; }
    auto begin() const noexcept { return dates_.begin(); }
    auto end() const noexcept { return dates_.end(); }

    friend bool operator==(const ObservationSchedule&, const ObservationSchedule&) = default;

private:
    explicit ObservationSchedule(std::vector<date> dates) : dates_(std::move(dates)) {}
    std::vector<date> dates_;

    friend result<ObservationSchedule> make_observation_schedule(
        std::vector<date>, date, date, const TradingCalendar&);
};

using observation_schedule = ObservationSchedule;
using Schedule = ObservationSchedule;

inline result<ObservationSchedule> make_observation_schedule(
    std::vector<date> observations, date instrument_start, date instrument_end,
    const TradingCalendar& calendar)
{
    auto valid = validate_observation_dates(observations, instrument_start, instrument_end, calendar);
    if (!valid) return std::unexpected(valid.error());
    return ObservationSchedule{std::move(observations)};
}

inline result<ObservationSchedule> make_schedule(
    std::vector<date> observations, date instrument_start, date instrument_end,
    const TradingCalendar& calendar)
{
    return make_observation_schedule(std::move(observations), instrument_start, instrument_end, calendar);
}

inline result<EuropeanOption> make_european_option(
    option_type type, double strike, date valuation_date, date expiry)
{
    auto valid_expiry = validate_expiry(valuation_date, expiry);
    if (!valid_expiry) return std::unexpected(valid_expiry.error());
    return make_european_option(type, strike, expiry);
}

inline result<void> validate_expiry(date valuation_date, date expiry)
{
    if (!is_valid_date(valuation_date) || !is_valid_date(expiry)) {
        return std::unexpected(Error{error_category::invalid_date,
                                     "valuation date and expiry must be valid calendar dates"});
    }
    if (expiry < valuation_date) {
        return std::unexpected(Error{error_category::invalid_expiry,
                                     "expiry must not precede the valuation date"});
    }
    return {};
}

inline result<EuropeanOption> make_european_call(double strike, date expiry)
{
    return make_european_option(option_type::call, strike, expiry);
}

inline result<EuropeanOption> make_european_put(double strike, date expiry)
{
    return make_european_option(option_type::put, strike, expiry);
}

class BsmParameters {
public:
    double risk_free_rate() const noexcept { return risk_free_rate_; }
    double rate() const noexcept { return risk_free_rate_; }
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

using bsm_parameters = BsmParameters;

inline result<BsmParameters> make_bsm_parameters(
    double risk_free_rate, double dividend_yield, double volatility)
{
    if (!std::isfinite(risk_free_rate) || !std::isfinite(dividend_yield)) {
        return std::unexpected(Error{error_category::invalid_parameter,
                                     "rates and dividend yield must be finite"});
    }
    if (!std::isfinite(volatility) || volatility <= 0.0) {
        return std::unexpected(Error{error_category::invalid_parameter,
                                     "volatility must be finite and positive"});
    }
    return BsmParameters{risk_free_rate, dividend_yield, volatility};
}

class AssetPrice {
public:
    double value() const noexcept { return value_; }
    operator double() const noexcept { return value_; }

    friend bool operator==(const AssetPrice&, const AssetPrice&) = default;

private:
    explicit AssetPrice(double value) : value_(value) {}
    double value_;
    friend result<AssetPrice> make_asset_price(double);
};

using asset_price = AssetPrice;

inline result<AssetPrice> make_asset_price(double value)
{
    if (!std::isfinite(value) || value <= 0.0) {
        return std::unexpected(Error{error_category::invalid_asset_price,
                                     "asset price must be finite and positive"});
    }
    return AssetPrice{value};
}

class PricingContext {
public:
    const BsmParameters& parameters() const noexcept { return parameters_; }
    const BsmParameters& bsm_parameters() const noexcept { return parameters_; }
    AssetPrice asset_price() const noexcept { return asset_price_; }
    date valuation_date() const noexcept { return valuation_date_; }
    const TradingCalendar& calendar() const noexcept { return calendar_; }
    const TradingCalendar& trading_calendar() const noexcept { return calendar_; }

    friend bool operator==(const PricingContext&, const PricingContext&) = default;

private:
    PricingContext(BsmParameters parameters, AssetPrice asset_price, date valuation_date,
                   TradingCalendar calendar)
        : parameters_(std::move(parameters)), asset_price_(asset_price), valuation_date_(valuation_date),
          calendar_(std::move(calendar)) {}

    BsmParameters parameters_;
    AssetPrice asset_price_;
    date valuation_date_;
    TradingCalendar calendar_;

    friend result<PricingContext> make_pricing_context(BsmParameters, AssetPrice, date, TradingCalendar);
};

using pricing_context = PricingContext;

inline result<PricingContext> make_pricing_context(
    BsmParameters, AssetPrice, date, TradingCalendar);

inline result<PricingContext> make_pricing_context(
    BsmParameters parameters, AssetPrice asset_price, date valuation_date)
{
    return make_pricing_context(std::move(parameters), asset_price, valuation_date, all_days_calendar());
}

inline result<PricingContext> make_pricing_context(
    BsmParameters parameters, AssetPrice asset_price, date valuation_date, TradingCalendar calendar)
{
    if (!is_valid_date(valuation_date)) {
        return std::unexpected(Error{error_category::invalid_date,
                                     "valuation date must be a valid calendar date"});
    }
    return PricingContext{std::move(parameters), asset_price, valuation_date, std::move(calendar)};
}

inline result<PricingContext> make_pricing_context(
    BsmParameters parameters, double asset, date valuation_date)
{
    auto validated_asset = make_asset_price(asset);
    if (!validated_asset) {
        return std::unexpected(validated_asset.error());
    }
    return make_pricing_context(std::move(parameters), *validated_asset, valuation_date);
}

inline result<PricingContext> make_pricing_context(
    BsmParameters parameters, double asset, date valuation_date, TradingCalendar calendar)
{
    auto validated_asset = make_asset_price(asset);
    if (!validated_asset) {
        return std::unexpected(validated_asset.error());
    }
    return make_pricing_context(std::move(parameters), *validated_asset, valuation_date, std::move(calendar));
}

inline result<PricingContext> make_pricing_context(
    BsmParameters parameters, AssetPrice asset, date valuation_date, const EuropeanOption& option)
{
    auto expiry = validate_expiry(valuation_date, option.expiry());
    if (!expiry) {
        return std::unexpected(expiry.error());
    }
    return make_pricing_context(std::move(parameters), asset, valuation_date);
}

inline result<PricingContext> make_pricing_context(
    BsmParameters parameters, AssetPrice asset, date valuation_date,
    const EuropeanOption& option, TradingCalendar calendar)
{
    auto expiry = validate_expiry(valuation_date, option.expiry());
    if (!expiry) return std::unexpected(expiry.error());
    return make_pricing_context(std::move(parameters), asset, valuation_date, std::move(calendar));
}

inline result<PricingContext> make_pricing_context(
    BsmParameters parameters, AssetPrice asset, date valuation_date,
    TradingCalendar calendar, const EuropeanOption& option)
{
    return make_pricing_context(std::move(parameters), asset, valuation_date, option, std::move(calendar));
}

inline result<PricingContext> make_pricing_context(
    BsmParameters parameters, double asset, date valuation_date, const EuropeanOption& option)
{
    auto validated_asset = make_asset_price(asset);
    if (!validated_asset) {
        return std::unexpected(validated_asset.error());
    }
    return make_pricing_context(std::move(parameters), *validated_asset, valuation_date, option);
}

inline result<PricingContext> make_pricing_context(
    BsmParameters parameters, double asset, date valuation_date,
    const EuropeanOption& option, TradingCalendar calendar)
{
    auto validated_asset = make_asset_price(asset);
    if (!validated_asset) return std::unexpected(validated_asset.error());
    return make_pricing_context(std::move(parameters), *validated_asset, valuation_date,
                                option, std::move(calendar));
}

inline result<PricingContext> make_pricing_context(
    BsmParameters parameters, double asset, date valuation_date,
    TradingCalendar calendar, const EuropeanOption& option)
{
    return make_pricing_context(std::move(parameters), asset, valuation_date, option, std::move(calendar));
}

struct PricingResult {
    /// Present value in the input asset-price currency units.
    const double value;
    /// Change in value per one unit of underlying price.
    const double delta;
    /// Change in delta per one unit of underlying price.
    const double gamma;
    /// Change in gamma per one unit of underlying price.
    const double speed;
    /// Per calendar day under Actual/365 Fixed.
    const double theta;
    /// Per calendar day under Actual/365 Fixed.
    const double charm;
    /// Per calendar day under Actual/365 Fixed.
    const double color;
    /// Per one percentage-point volatility move.
    const double vega;
    /// Per one percentage-point volatility move.
    const double vanna;
    /// Per one percentage-point volatility move.
    const double zomma;
    /// Per one percentage-point rate move.
    const double rho;

    double price() const noexcept { return value; }
};

using pricing_result = PricingResult;

struct ImpliedVolatilitySettings {
    double lower_bound = 0.0001;
    double upper_bound = 4.0;
    double tolerance = 1e-8;
    int max_iterations = 100;
};

class AnalyticEuropeanEngine {
public:
    /// Returns intrinsic value and zero Greeks when valued at expiry.
    result<PricingResult> price(const EuropeanOption& option, const PricingContext& context) const;

    result<double> implied_volatility(
        const EuropeanOption& option, const PricingContext& context, double observed_price,
        ImpliedVolatilitySettings settings = {}) const;
};

using analytic_european_engine = AnalyticEuropeanEngine;

struct BinomialAmericanSettings {
    int steps = 256;
};

using binomial_american_settings = BinomialAmericanSettings;

/// Cox-Ross-Rubinstein American engine.
/// Value is tree-derived; delta and gamma are numerical tree estimates; higher Greeks are unsupported and zero.
class BinomialAmericanEngine {
public:
    explicit BinomialAmericanEngine(BinomialAmericanSettings settings = {}) : settings_(settings) {}
    explicit BinomialAmericanEngine(int steps) : settings_{steps} {}

    result<PricingResult> price(const EuropeanOption&, const PricingContext&) const;
    result<PricingResult> price(const EuropeanOption&, const PricingContext&, BinomialAmericanSettings) const;
    result<PricingResult> price(const EuropeanOption& option, const PricingContext& context, int steps) const
    { return price(option, context, BinomialAmericanSettings{steps}); }

    BinomialAmericanSettings settings() const noexcept { return settings_; }

private:
    BinomialAmericanSettings settings_;
};

using binomial_american_engine = BinomialAmericanEngine;

class CashOrNothingOption {
public:
    option_type type() const noexcept { return type_; }
    double strike() const noexcept { return strike_; }
    double payout() const noexcept { return payout_; }
    date expiry() const noexcept { return expiry_; }
    date expiration() const noexcept { return expiry_; }
    friend bool operator==(const CashOrNothingOption&, const CashOrNothingOption&) = default;
private:
    CashOrNothingOption(option_type type, double strike, double payout, date expiry)
        : type_(type), strike_(strike), payout_(payout), expiry_(expiry) {}
    option_type type_;
    double strike_;
    double payout_;
    date expiry_;
    friend result<CashOrNothingOption> make_cash_or_nothing_option(option_type, double, double, date);
};
using cash_or_nothing_option = CashOrNothingOption;
using CashDigitalOption = CashOrNothingOption;
using DigitalCashOption = CashOrNothingOption;

inline result<CashOrNothingOption> make_cash_or_nothing_option(option_type, double, double, date);

inline result<CashOrNothingOption> make_cash_or_nothing_call(double strike, double payout, date expiry)
{ return make_cash_or_nothing_option(option_type::call, strike, payout, expiry); }
inline result<CashOrNothingOption> make_cash_or_nothing_put(double strike, double payout, date expiry)
{ return make_cash_or_nothing_option(option_type::put, strike, payout, expiry); }

inline result<CashOrNothingOption> make_cash_or_nothing_option(
    option_type type, double strike, double payout, date expiry)
{
    if (type != option_type::call && type != option_type::put)
        return std::unexpected(Error{error_category::invalid_option, "option type must be call or put"});
    if (!std::isfinite(strike) || strike <= 0.0 || !std::isfinite(payout) || payout <= 0.0)
        return std::unexpected(Error{error_category::invalid_option, "strike and payout must be finite and positive"});
    if (!is_valid_date(expiry))
        return std::unexpected(Error{error_category::invalid_date, "expiry must be a valid calendar date"});
    return CashOrNothingOption{type, strike, payout, expiry};
}

inline result<CashOrNothingOption> make_cash_or_nothing_option(
    option_type type, double strike, date expiry, double payout)
{ return make_cash_or_nothing_option(type, strike, payout, expiry); }

inline result<CashOrNothingOption> make_cash_or_nothing_option(
    option_type type, double strike, double payout, date valuation, date expiry)
{
    auto valid = validate_expiry(valuation, expiry);
    if (!valid) return std::unexpected(valid.error());
    return make_cash_or_nothing_option(type, strike, payout, expiry);
}

class AssetOrNothingOption {
public:
    option_type type() const noexcept { return type_; }
    double strike() const noexcept { return strike_; }
    date expiry() const noexcept { return expiry_; }
    date expiration() const noexcept { return expiry_; }
    friend bool operator==(const AssetOrNothingOption&, const AssetOrNothingOption&) = default;
private:
    AssetOrNothingOption(option_type type, double strike, date expiry)
        : type_(type), strike_(strike), expiry_(expiry) {}
    option_type type_;
    double strike_;
    date expiry_;
    friend result<AssetOrNothingOption> make_asset_or_nothing_option(option_type, double, date);
};
using asset_or_nothing_option = AssetOrNothingOption;
using AssetDigitalOption = AssetOrNothingOption;
using DigitalAssetOption = AssetOrNothingOption;

inline result<AssetOrNothingOption> make_asset_or_nothing_option(option_type, double, date);

inline result<AssetOrNothingOption> make_asset_or_nothing_call(double strike, date expiry)
{ return make_asset_or_nothing_option(option_type::call, strike, expiry); }
inline result<AssetOrNothingOption> make_asset_or_nothing_put(double strike, date expiry)
{ return make_asset_or_nothing_option(option_type::put, strike, expiry); }

inline result<AssetOrNothingOption> make_asset_or_nothing_option(option_type type, double strike, date expiry)
{
    if (type != option_type::call && type != option_type::put)
        return std::unexpected(Error{error_category::invalid_option, "option type must be call or put"});
    if (!std::isfinite(strike) || strike <= 0.0)
        return std::unexpected(Error{error_category::invalid_option, "strike must be finite and positive"});
    if (!is_valid_date(expiry))
        return std::unexpected(Error{error_category::invalid_date, "expiry must be a valid calendar date"});
    return AssetOrNothingOption{type, strike, expiry};
}

inline result<AssetOrNothingOption> make_asset_or_nothing_option(
    option_type type, double strike, date valuation, date expiry)
{
    auto valid = validate_expiry(valuation, expiry);
    if (!valid) return std::unexpected(valid.error());
    return make_asset_or_nothing_option(type, strike, expiry);
}

enum class barrier_type {
    up_and_in, up_and_out, down_and_in, down_and_out,
    UpAndIn = up_and_in, UpAndOut = up_and_out, DownAndIn = down_and_in, DownAndOut = down_and_out
};
using BarrierType = barrier_type;
enum class observation_mode { continuous, scheduled, Continuous = continuous, Scheduled = scheduled };
using ObservationMode = observation_mode;
enum class rebate_timing { at_hit, at_expiry, AtHit = at_hit, AtExpiry = at_expiry };
using RebateTiming = rebate_timing;

class BarrierOption {
public:
    option_type type() const noexcept { return type_; }
    double strike() const noexcept { return strike_; }
    double barrier() const noexcept { return barrier_; }
    barrier_type barrier_kind() const noexcept { return kind_; }
    barrier_type kind() const noexcept { return kind_; }
    double rebate() const noexcept { return rebate_; }
    ito::rebate_timing rebate_payment() const noexcept { return timing_; }
    ito::rebate_timing rebate_timing() const noexcept { return timing_; }
    observation_mode observation() const noexcept { return observation_; }
    observation_mode observation_mode_value() const noexcept { return observation_; }
    const std::vector<date>& observation_dates() const noexcept { return observations_; }
    date expiry() const noexcept { return expiry_; }
    date expiration() const noexcept { return expiry_; }
    friend bool operator==(const BarrierOption&, const BarrierOption&) = default;
private:
    BarrierOption(option_type type, double strike, date expiry, double barrier, barrier_type kind,
                  double rebate, ito::rebate_timing timing, observation_mode observation,
                  std::vector<date> observations)
        : type_(type), strike_(strike), barrier_(barrier), kind_(kind), rebate_(rebate), timing_(timing),
          observation_(observation), observations_(std::move(observations)), expiry_(expiry) {}
    option_type type_;
    double strike_;
    double barrier_;
    barrier_type kind_;
    double rebate_;
    ito::rebate_timing timing_;
    observation_mode observation_;
    std::vector<date> observations_;
    date expiry_;
    friend result<BarrierOption> make_barrier_option(option_type, double, date, double, barrier_type,
                                                     double, ito::rebate_timing, observation_mode, std::vector<date>);
};
using barrier_option = BarrierOption;

inline result<BarrierOption> make_barrier_option(
    option_type type, double strike, date expiry, double barrier, barrier_type kind,
    double rebate = 0.0, rebate_timing timing = rebate_timing::at_expiry,
    observation_mode observation = observation_mode::continuous,
    std::vector<date> observations = {})
{
    if (type != option_type::call && type != option_type::put)
        return std::unexpected(Error{error_category::invalid_option, "option type must be call or put"});
    if (!std::isfinite(strike) || strike <= 0.0 || !std::isfinite(barrier) || barrier <= 0.0 ||
        !std::isfinite(rebate) || rebate < 0.0)
        return std::unexpected(Error{error_category::invalid_option, "barrier terms must be finite and non-negative"});
    if (!is_valid_date(expiry))
        return std::unexpected(Error{error_category::invalid_date, "expiry must be a valid calendar date"});
    if (kind != barrier_type::up_and_in && kind != barrier_type::up_and_out &&
        kind != barrier_type::down_and_in && kind != barrier_type::down_and_out)
        return std::unexpected(Error{error_category::invalid_option, "invalid barrier type"});
    if (timing != rebate_timing::at_hit && timing != rebate_timing::at_expiry)
        return std::unexpected(Error{error_category::invalid_option, "invalid rebate timing"});
    if (observation != observation_mode::continuous && observation != observation_mode::scheduled)
        return std::unexpected(Error{error_category::invalid_schedule, "invalid observation mode"});
    const bool knock_in = kind == barrier_type::up_and_in || kind == barrier_type::down_and_in;
    if (knock_in && timing == rebate_timing::at_hit)
        return std::unexpected(Error{error_category::invalid_option, "at-hit rebates are invalid for knock-in barriers"});
    if (observation == observation_mode::continuous && !observations.empty())
        return std::unexpected(Error{error_category::invalid_schedule, "continuous barriers cannot have observations"});
    if (observation == observation_mode::scheduled) {
        if (observations.empty())
            return std::unexpected(Error{error_category::invalid_schedule, "scheduled barriers require observations"});
        for (std::size_t i = 0; i < observations.size(); ++i) {
            if (!is_valid_date(observations[i]) || (i && observations[i] <= observations[i - 1]) || observations[i] > expiry)
                return std::unexpected(Error{error_category::invalid_schedule, "observation dates must be ordered and precede expiry"});
        }
    }
    return BarrierOption{type, strike, expiry, barrier, kind, rebate, timing, observation, std::move(observations)};
}

inline result<BarrierOption> make_barrier_option(
    option_type type, double strike, double barrier, date expiry, barrier_type kind,
    double rebate = 0.0, rebate_timing timing = rebate_timing::at_expiry,
    observation_mode observation = observation_mode::continuous,
    std::vector<date> observations = {})
{ return make_barrier_option(type, strike, expiry, barrier, kind, rebate, timing, observation, std::move(observations)); }

class AnalyticDigitalEngine {
public:
    result<PricingResult> price(const CashOrNothingOption&, const PricingContext&) const;
    result<PricingResult> price(const AssetOrNothingOption&, const PricingContext&) const;
};
using analytic_digital_engine = AnalyticDigitalEngine;

class AnalyticBarrierEngine {
public:
    result<PricingResult> price(const BarrierOption&, const PricingContext&) const;
};
using analytic_barrier_engine = AnalyticBarrierEngine;

}
