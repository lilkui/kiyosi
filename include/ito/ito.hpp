#pragma once

#include <chrono>
#include <cmath>
#include <cstdint>
#include <expected>
#include <functional>
#include <optional>
#include <span>
#include <string_view>
#include <utility>
#include <vector>

namespace ito {

inline constexpr int version_major = 0;
inline constexpr int version_minor = 1;
inline constexpr int version_patch = 0;

using date = std::chrono::sys_days;
// Intraday moments use UTC-like sys_time; date-based contracts remain midnight anchored.
using timestamp = std::chrono::sys_time<std::chrono::nanoseconds>;
using time_point = timestamp;
[[nodiscard]] inline bool is_valid_date(date value) noexcept;

enum class day_count_convention : unsigned char {
    actual_365_fixed,
};

[[nodiscard]] inline timestamp start_of_day(date value) noexcept
{
    return timestamp{value.time_since_epoch()};
}

[[nodiscard]] inline date date_of(timestamp value) noexcept
{
    return date{std::chrono::floor<std::chrono::days>(value.time_since_epoch())};
}


enum class error_category : unsigned char {
    invalid_option = 1,
    invalid_strike = 2,
    invalid_volatility = 3,
    invalid_rate = 4,
    invalid_dividend = 5,
    invalid_asset_price = 6,
    invalid_date = 7,
    invalid_expiry = 8,
    invalid_result = 9,
    invalid_schedule = 10,
    invalid_calendar = 11,
    invalid_parameter = 12,
    incompatible_exercise = 13,
    unsupported_risk_measure = 14,
    invalid_quote = 15,
    unbracketed_volatility = 16,
    solver_non_convergence = 17,
    solver_non_finite = 18,
};

struct Error {
    error_category category;
    std::string_view message;

    friend bool operator==(const Error&, const Error&) = default;
};

template <typename T>
using result = std::expected<T, Error>;

[[nodiscard]] inline result<double> year_fraction(
    date start, date end, day_count_convention convention = day_count_convention::actual_365_fixed)
{
    if (!is_valid_date(start) || !is_valid_date(end))
        return std::unexpected(Error{error_category::invalid_date, "day-count dates must be valid calendar dates"});
    if (end < start)
        return std::unexpected(Error{error_category::invalid_expiry, "day-count end must not precede start"});
    if (convention != day_count_convention::actual_365_fixed)
        return std::unexpected(Error{error_category::invalid_parameter, "unsupported day-count convention"});
    return static_cast<double>((end - start).count()) / 365.0;
}

[[nodiscard]] inline result<double> year_fraction(
    timestamp start, timestamp end, day_count_convention convention = day_count_convention::actual_365_fixed)
{
    if (!is_valid_date(date_of(start)) || !is_valid_date(date_of(end)))
        return std::unexpected(Error{error_category::invalid_date, "day-count timestamps must contain valid dates"});
    if (end < start)
        return std::unexpected(Error{error_category::invalid_expiry, "day-count end must not precede start"});
    if (convention != day_count_convention::actual_365_fixed)
        return std::unexpected(Error{error_category::invalid_parameter, "unsupported day-count convention"});
    return std::chrono::duration<double, std::ratio<86400 * 365>>{end - start}.count();
}

enum class option_type {
    call,
    put,
};

class EuropeanOption {
public:
    option_type type() const noexcept { return type_; }
    double strike() const noexcept { return strike_; }
    date expiry() const noexcept { return expiry_; }

    friend bool operator==(const EuropeanOption&, const EuropeanOption&) = default;

private:
    EuropeanOption(option_type type, double strike, date expiry)
        : type_(type), strike_(strike), expiry_(expiry) {}

    option_type type_;
    double strike_;
    date expiry_;

    friend result<EuropeanOption> make_european_option(option_type, double, date);
};

class AmericanOption {
public:
    option_type type() const noexcept { return type_; }
    double strike() const noexcept { return strike_; }
    date expiry() const noexcept { return expiry_; }

    friend bool operator==(const AmericanOption&, const AmericanOption&) = default;

private:
    AmericanOption(option_type type, double strike, date expiry)
        : type_(type), strike_(strike), expiry_(expiry) {}

    option_type type_;
    double strike_;
    date expiry_;

    friend result<AmericanOption> make_american_option(option_type, double, date);
};

[[nodiscard]] inline result<EuropeanOption> make_european_option(option_type type, double strike, date expiry)
{
    if (type != option_type::call && type != option_type::put) {
        return std::unexpected(Error{error_category::invalid_option, "option type must be call or put"});
    }
    if (!std::isfinite(strike) || strike <= 0.0) {
        return std::unexpected(Error{error_category::invalid_strike, "strike must be finite and positive"});
    }
    if (!is_valid_date(expiry)) {
        return std::unexpected(Error{error_category::invalid_date, "expiry must be a valid calendar date"});
    }
    return EuropeanOption{type, strike, expiry};
}

[[nodiscard]] inline result<AmericanOption> make_american_option(option_type type, double strike, date expiry)
{
    if (type != option_type::call && type != option_type::put)
        return std::unexpected(Error{error_category::invalid_option, "option type must be call or put"});
    if (!std::isfinite(strike) || strike <= 0.0)
        return std::unexpected(Error{error_category::invalid_strike, "strike must be finite and positive"});
    if (!is_valid_date(expiry))
        return std::unexpected(Error{error_category::invalid_date, "expiry must be a valid calendar date"});
    return AmericanOption{type, strike, expiry};
}

[[nodiscard]] inline result<void> validate_expiry(date valuation_date, date expiry);
[[nodiscard]] inline result<void> validate_expiry(timestamp valuation_time, date expiry);

[[nodiscard]] inline bool is_valid_date(date value) noexcept
{
    return std::chrono::year_month_day{value}.ok();
}

class TradingCalendar {
public:
    using trading_day_predicate = std::function<bool(date)>;

    [[nodiscard]] bool is_trading_day(date value) const
    {
        return is_valid_date(value) && predicate_ && predicate_(value);
    }

    int annual_trading_days() const noexcept { return annual_trading_days_; }

    [[nodiscard]] int trading_days_between(date start, date end) const
    {
        if (!is_valid_date(start) || !is_valid_date(end) || end < start) return 0;
        int count = 0;
        for (auto value = start; value < end; value += std::chrono::days{1})
            count += is_trading_day(value) ? 1 : 0;
        return count;
    }

    [[nodiscard]] double trading_year_fraction(date start, date end) const
    {
        return static_cast<double>(trading_days_between(start, end)) /
               static_cast<double>(annual_trading_days_);
    }

private:
    TradingCalendar(trading_day_predicate predicate, int annual_trading_days)
        : predicate_(std::move(predicate)), annual_trading_days_(annual_trading_days) {}

    friend result<TradingCalendar> make_trading_calendar(trading_day_predicate, int);
    friend TradingCalendar all_days_calendar();
    friend TradingCalendar exchange_calendar();

    trading_day_predicate predicate_;
    int annual_trading_days_;
};

[[nodiscard]] inline result<TradingCalendar> make_trading_calendar(
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

[[nodiscard]] inline TradingCalendar all_days_calendar()
{
    return TradingCalendar{[](date) { return true; }, 365};
}

[[nodiscard]] inline TradingCalendar exchange_calendar()
{
    return TradingCalendar{[](date value) {
                               const auto weekday = std::chrono::weekday{value};
                               return weekday != std::chrono::Saturday && weekday != std::chrono::Sunday;
                           },
                           252};
}

[[nodiscard]] inline result<void> validate_observation_date(
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

[[nodiscard]] inline result<void> validate_observation_dates(
    std::span<const date> observations, date instrument_start, date instrument_end,
    const TradingCalendar& calendar)
{
    if (!is_valid_date(instrument_start) || !is_valid_date(instrument_end) || instrument_end < instrument_start) {
        return std::unexpected(Error{error_category::invalid_date,
                                     "instrument life must be a valid ordered date range"});
    }
    for (std::size_t index = 0; index < observations.size(); ++index) {
        if (index > 0 && observations[index] <= observations[index - 1]) {
            return std::unexpected(Error{error_category::invalid_date,
                                         "observation dates must be strictly ordered"});
        }
        auto valid = validate_observation_date(observations[index], instrument_start, instrument_end, calendar);
        if (!valid) return std::unexpected(valid.error());
    }
    return {};
}

[[nodiscard]] inline result<void> validate_schedule(
    std::span<const date> observations, date instrument_start, date instrument_end,
    const TradingCalendar& calendar)
{
    return validate_observation_dates(observations, instrument_start, instrument_end, calendar);
}

[[nodiscard]] inline result<void> validate_observation_dates(
    std::span<const date> observations, date valuation_date, const EuropeanOption& option,
    const TradingCalendar& calendar)
{
    return validate_observation_dates(observations, valuation_date, option.expiry(), calendar);
}

[[nodiscard]] inline result<void> validate_schedule(
    std::span<const date> observations, date valuation_date, const EuropeanOption& option,
    const TradingCalendar& calendar)
{
    return validate_observation_dates(observations, valuation_date, option, calendar);
}

[[nodiscard]] inline result<void> validate_observation_dates(
    std::span<const date> observations, date valuation_date, const AmericanOption& option,
    const TradingCalendar& calendar)
{
    return validate_observation_dates(observations, valuation_date, option.expiry(), calendar);
}

[[nodiscard]] inline result<void> validate_schedule(
    std::span<const date> observations, date valuation_date, const AmericanOption& option,
    const TradingCalendar& calendar)
{
    return validate_observation_dates(observations, valuation_date, option, calendar);
}

class ObservationSchedule {
public:
    const std::vector<date>& dates() const noexcept { return dates_; }
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

[[nodiscard]] inline result<void> validate_schedule(
    const ObservationSchedule& schedule, date instrument_start, date instrument_end,
    const TradingCalendar& calendar)
{
    return validate_observation_dates(schedule.dates(), instrument_start, instrument_end, calendar);
}

[[nodiscard]] inline result<ObservationSchedule> make_observation_schedule(
    std::vector<date> observations, date instrument_start, date instrument_end,
    const TradingCalendar& calendar)
{
    auto valid = validate_observation_dates(observations, instrument_start, instrument_end, calendar);
    if (!valid) return std::unexpected(valid.error());
    return ObservationSchedule{std::move(observations)};
}

[[nodiscard]] inline result<EuropeanOption> make_european_option(
    option_type type, double strike, date valuation_date, date expiry)
{
    auto valid_expiry = validate_expiry(valuation_date, expiry);
    if (!valid_expiry) return std::unexpected(valid_expiry.error());
    return make_european_option(type, strike, expiry);
}

[[nodiscard]] inline result<AmericanOption> make_american_option(
    option_type type, double strike, date valuation_date, date expiry)
{
    auto valid_expiry = validate_expiry(valuation_date, expiry);
    if (!valid_expiry) return std::unexpected(valid_expiry.error());
    return make_american_option(type, strike, expiry);
}

[[nodiscard]] inline result<void> validate_expiry(date valuation_date, date expiry)
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

[[nodiscard]] inline result<EuropeanOption> make_european_call(double strike, date expiry)
{
    return make_european_option(option_type::call, strike, expiry);
}

[[nodiscard]] inline result<EuropeanOption> make_european_put(double strike, date expiry)
{
    return make_european_option(option_type::put, strike, expiry);
}

[[nodiscard]] inline result<AmericanOption> make_american_call(double strike, date expiry)
{
    return make_american_option(option_type::call, strike, expiry);
}

[[nodiscard]] inline result<AmericanOption> make_american_put(double strike, date expiry)
{
    return make_american_option(option_type::put, strike, expiry);
}

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
    const BsmParameters& parameters() const noexcept { return parameters_; }
    AssetPrice asset_price() const noexcept { return asset_price_; }
    date valuation_date() const noexcept { return date_of(valuation_time_); }
    timestamp valuation_time() const noexcept { return valuation_time_; }
    timestamp valuation_moment() const noexcept { return valuation_time_; }
    const TradingCalendar& calendar() const noexcept { return calendar_; }

private:
    PricingContext(BsmParameters parameters, AssetPrice asset_price, timestamp valuation_time,
                   TradingCalendar calendar)
        : parameters_(std::move(parameters)), asset_price_(asset_price), valuation_time_(valuation_time),
          calendar_(std::move(calendar)) {}

    BsmParameters parameters_;
    AssetPrice asset_price_;
    timestamp valuation_time_;
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

[[nodiscard]] inline result<void> validate_expiry(timestamp valuation_time, date expiry)
{
    if (!is_valid_date(date_of(valuation_time)) || !is_valid_date(expiry))
        return std::unexpected(Error{error_category::invalid_date,
                                     "valuation time and expiry must contain valid calendar dates"});
    if (date_of(valuation_time) > expiry)
        return std::unexpected(Error{error_category::invalid_expiry,
                                     "expiry must not precede the valuation time"});
    return {};
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

enum class risk_measure : std::uint16_t {
    price = 1u << 0,
    delta = 1u << 1,
    gamma = 1u << 2,
    speed = 1u << 3,
    theta = 1u << 4,
    charm = 1u << 5,
    color = 1u << 6,
    vega = 1u << 7,
    vanna = 1u << 8,
    zomma = 1u << 9,
    rho = 1u << 10,
};

using risk_measure_set = std::uint16_t;

[[nodiscard]] constexpr risk_measure_set risk_bit(risk_measure measure) noexcept
{
    return static_cast<risk_measure_set>(measure);
}

[[nodiscard]] constexpr risk_measure_set operator|(risk_measure left, risk_measure right) noexcept
{
    return risk_bit(left) | risk_bit(right);
}

[[nodiscard]] constexpr risk_measure_set operator|(risk_measure_set left, risk_measure right) noexcept
{
    return left | risk_bit(right);
}

[[nodiscard]] constexpr risk_measure_set operator|(risk_measure left, risk_measure_set right) noexcept
{
    return risk_bit(left) | right;
}

inline constexpr risk_measure_set all_risk_measures =
    risk_bit(risk_measure::price) | risk_bit(risk_measure::delta) | risk_bit(risk_measure::gamma) |
    risk_bit(risk_measure::speed) | risk_bit(risk_measure::theta) | risk_bit(risk_measure::charm) |
    risk_bit(risk_measure::color) | risk_bit(risk_measure::vega) | risk_bit(risk_measure::vanna) |
    risk_bit(risk_measure::zomma) | risk_bit(risk_measure::rho);

struct PricingResult {
    /// Present value in the input asset-price currency units.
    double value;
    /// Change in value per one unit of underlying price.
    double delta;
    /// Change in delta per one unit of underlying price.
    double gamma;
    /// Change in gamma per one unit of underlying price.
    double speed;
    /// Per calendar day under Actual/365 Fixed.
    double theta;
    /// Per calendar day under Actual/365 Fixed.
    double charm;
    /// Per calendar day under Actual/365 Fixed.
    double color;
    /// Per one percentage-point volatility move.
    double vega;
    /// Per one percentage-point volatility move.
    double vanna;
    /// Per one percentage-point volatility move.
    double zomma;
    /// Per one percentage-point rate move.
    double rho;

    risk_measure_set available = all_risk_measures;

    [[nodiscard]] bool has(risk_measure measure) const noexcept;
    [[nodiscard]] std::optional<double> get(risk_measure measure) const noexcept;
};

struct PricingRequest {
    risk_measure_set measures = all_risk_measures;

    [[nodiscard]] static constexpr PricingRequest price_only() noexcept
    {
        return PricingRequest{risk_bit(risk_measure::price)};
    }
    [[nodiscard]] static constexpr PricingRequest all() noexcept { return PricingRequest{}; }
    [[nodiscard]] constexpr bool requests(risk_measure measure) const noexcept
    {
        return (measures & risk_bit(measure)) != 0;
    }
};

[[nodiscard]] inline bool PricingResult::has(risk_measure measure) const noexcept
{
    return (available & risk_bit(measure)) != 0;
}

[[nodiscard]] inline std::optional<double> PricingResult::get(risk_measure measure) const noexcept
{
    if (!has(measure)) return std::nullopt;
    switch (measure) {
    case risk_measure::price:
        return value;
    case risk_measure::delta:
        return delta;
    case risk_measure::gamma:
        return gamma;
    case risk_measure::speed:
        return speed;
    case risk_measure::theta:
        return theta;
    case risk_measure::charm:
        return charm;
    case risk_measure::color:
        return color;
    case risk_measure::vega:
        return vega;
    case risk_measure::vanna:
        return vanna;
    case risk_measure::zomma:
        return zomma;
    case risk_measure::rho:
        return rho;
    }
    return std::nullopt;
}

struct ImpliedVolatilitySettings {
    double lower_bound = 0.0001;
    double upper_bound = 4.0;
    double tolerance = 1e-8;
    int max_iterations = 100;
};

class AnalyticEuropeanEngine {
public:
    static constexpr risk_measure_set supported_risk_measures = all_risk_measures;

    /// Returns intrinsic value and zero Greeks when valued at expiry.
    [[nodiscard]] result<PricingResult> price(const EuropeanOption& option, const PricingContext& context) const;
    [[nodiscard]] result<PricingResult> price(
        const EuropeanOption& option, const PricingContext& context, PricingRequest request) const;

    [[nodiscard]] result<double> implied_volatility(
        const EuropeanOption& option, const PricingContext& context, double observed_price,
        ImpliedVolatilitySettings settings = {}) const;
};

struct BinomialAmericanSettings {
    int steps = 256;
};

/// Cox-Ross-Rubinstein American engine.
/// Value is tree-derived; delta and gamma are numerical tree estimates; higher Greeks are unsupported and zero.
class BinomialAmericanEngine {
public:
    static constexpr risk_measure_set supported_risk_measures =
        risk_bit(risk_measure::price) | risk_bit(risk_measure::delta) | risk_bit(risk_measure::gamma);

    explicit BinomialAmericanEngine(BinomialAmericanSettings settings = {}) : settings_(settings) {}
    explicit BinomialAmericanEngine(int steps) : settings_{steps} {}

    [[nodiscard]] result<PricingResult> price(const EuropeanOption&, const PricingContext&) const;
    [[nodiscard]] result<PricingResult> price(const EuropeanOption&, const PricingContext&, BinomialAmericanSettings) const;
    [[nodiscard]] result<PricingResult> price(const AmericanOption&, const PricingContext&) const;
    [[nodiscard]] result<PricingResult> price(const AmericanOption&, const PricingContext&, PricingRequest) const;
    [[nodiscard]] result<PricingResult> price(const AmericanOption&, const PricingContext&, BinomialAmericanSettings) const;
    [[nodiscard]] result<PricingResult> price(
        const AmericanOption&, const PricingContext&, BinomialAmericanSettings, PricingRequest) const;
    [[nodiscard]] result<PricingResult> price(
        const EuropeanOption&, const PricingContext&, PricingRequest) const;

    BinomialAmericanSettings settings() const noexcept { return settings_; }

private:
    BinomialAmericanSettings settings_;
};

enum class finite_difference_scheme : unsigned char {
    explicit_euler,
    implicit_euler,
    crank_nicolson,
};

struct FiniteDifferenceSettings {
    int asset_steps = 200;
    int time_steps = 200;
    finite_difference_scheme scheme = finite_difference_scheme::crank_nicolson;
    double upper_boundary = 0.0;
};

/// Uniform-grid finite-difference European engine for vanilla options.
class FiniteDifferenceEuropeanEngine {
public:
    static constexpr risk_measure_set supported_risk_measures =
        risk_bit(risk_measure::price) | risk_bit(risk_measure::delta) | risk_bit(risk_measure::gamma);

    explicit FiniteDifferenceEuropeanEngine(FiniteDifferenceSettings settings = {})
        : settings_(settings) {}
    FiniteDifferenceEuropeanEngine(int asset_steps, int time_steps,
                                   finite_difference_scheme scheme = finite_difference_scheme::crank_nicolson)
        : settings_{asset_steps, time_steps, scheme} {}

    [[nodiscard]] result<PricingResult> price(const EuropeanOption&, const PricingContext&) const;
    [[nodiscard]] result<PricingResult> price(const EuropeanOption&, const PricingContext&, FiniteDifferenceSettings) const;
    [[nodiscard]] result<PricingResult> price(
        const EuropeanOption&, const PricingContext&, PricingRequest) const;
    [[nodiscard]] result<PricingResult> price(
        const EuropeanOption&, const PricingContext&, FiniteDifferenceSettings, PricingRequest) const;
    FiniteDifferenceSettings settings() const noexcept { return settings_; }

private:
    FiniteDifferenceSettings settings_;
};

/// Uniform-grid finite-difference American engine with early exercise at every time layer.
class FiniteDifferenceAmericanEngine {
public:
    static constexpr risk_measure_set supported_risk_measures =
        risk_bit(risk_measure::price) | risk_bit(risk_measure::delta) | risk_bit(risk_measure::gamma);

    explicit FiniteDifferenceAmericanEngine(FiniteDifferenceSettings settings = {})
        : settings_(settings) {}
    FiniteDifferenceAmericanEngine(int asset_steps, int time_steps,
                                   finite_difference_scheme scheme = finite_difference_scheme::crank_nicolson)
        : settings_{asset_steps, time_steps, scheme} {}

    [[nodiscard]] result<PricingResult> price(const EuropeanOption&, const PricingContext&) const;
    [[nodiscard]] result<PricingResult> price(const EuropeanOption&, const PricingContext&, FiniteDifferenceSettings) const;
    [[nodiscard]] result<PricingResult> price(const AmericanOption&, const PricingContext&) const;
    [[nodiscard]] result<PricingResult> price(const AmericanOption&, const PricingContext&, PricingRequest) const;
    [[nodiscard]] result<PricingResult> price(const AmericanOption&, const PricingContext&, FiniteDifferenceSettings) const;
    [[nodiscard]] result<PricingResult> price(
        const AmericanOption&, const PricingContext&, FiniteDifferenceSettings, PricingRequest) const;
    [[nodiscard]] result<PricingResult> price(
        const EuropeanOption&, const PricingContext&, PricingRequest) const;
    FiniteDifferenceSettings settings() const noexcept { return settings_; }

private:
    FiniteDifferenceSettings settings_;
};

class CashOrNothingOption {
public:
    option_type type() const noexcept { return type_; }
    double strike() const noexcept { return strike_; }
    double payout() const noexcept { return payout_; }
    date expiry() const noexcept { return expiry_; }
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
[[nodiscard]] inline result<CashOrNothingOption> make_cash_or_nothing_option(option_type, double, double, date);

[[nodiscard]] inline result<CashOrNothingOption> make_cash_or_nothing_call(double strike, double payout, date expiry)
{
    return make_cash_or_nothing_option(option_type::call, strike, payout, expiry);
}
[[nodiscard]] inline result<CashOrNothingOption> make_cash_or_nothing_put(double strike, double payout, date expiry)
{
    return make_cash_or_nothing_option(option_type::put, strike, payout, expiry);
}

[[nodiscard]] inline result<CashOrNothingOption> make_cash_or_nothing_option(
    option_type type, double strike, double payout, date expiry)
{
    if (type != option_type::call && type != option_type::put)
        return std::unexpected(Error{error_category::invalid_option, "option type must be call or put"});
    if (!std::isfinite(strike) || strike <= 0.0)
        return std::unexpected(Error{error_category::invalid_strike, "strike must be finite and positive"});
    if (!std::isfinite(payout) || payout <= 0.0)
        return std::unexpected(Error{error_category::invalid_parameter, "payout must be finite and positive"});
    if (!is_valid_date(expiry))
        return std::unexpected(Error{error_category::invalid_date, "expiry must be a valid calendar date"});
    return CashOrNothingOption{type, strike, payout, expiry};
}

[[nodiscard]] inline result<CashOrNothingOption> make_cash_or_nothing_option(
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
    friend bool operator==(const AssetOrNothingOption&, const AssetOrNothingOption&) = default;

private:
    AssetOrNothingOption(option_type type, double strike, date expiry)
        : type_(type), strike_(strike), expiry_(expiry) {}
    option_type type_;
    double strike_;
    date expiry_;
    friend result<AssetOrNothingOption> make_asset_or_nothing_option(option_type, double, date);
};
[[nodiscard]] inline result<AssetOrNothingOption> make_asset_or_nothing_option(option_type, double, date);

[[nodiscard]] inline result<AssetOrNothingOption> make_asset_or_nothing_call(double strike, date expiry)
{
    return make_asset_or_nothing_option(option_type::call, strike, expiry);
}
[[nodiscard]] inline result<AssetOrNothingOption> make_asset_or_nothing_put(double strike, date expiry)
{
    return make_asset_or_nothing_option(option_type::put, strike, expiry);
}

[[nodiscard]] inline result<AssetOrNothingOption> make_asset_or_nothing_option(option_type type, double strike, date expiry)
{
    if (type != option_type::call && type != option_type::put)
        return std::unexpected(Error{error_category::invalid_option, "option type must be call or put"});
    if (!std::isfinite(strike) || strike <= 0.0)
        return std::unexpected(Error{error_category::invalid_strike, "strike must be finite and positive"});
    if (!is_valid_date(expiry))
        return std::unexpected(Error{error_category::invalid_date, "expiry must be a valid calendar date"});
    return AssetOrNothingOption{type, strike, expiry};
}

[[nodiscard]] inline result<AssetOrNothingOption> make_asset_or_nothing_option(
    option_type type, double strike, date valuation, date expiry)
{
    auto valid = validate_expiry(valuation, expiry);
    if (!valid) return std::unexpected(valid.error());
    return make_asset_or_nothing_option(type, strike, expiry);
}

enum class barrier_type {
    up_and_in,
    up_and_out,
    down_and_in,
    down_and_out,
};
enum class observation_mode { continuous,
                              scheduled };
enum class rebate_timing { at_hit,
                           at_expiry };

class BarrierOption {
public:
    option_type type() const noexcept { return type_; }
    double strike() const noexcept { return strike_; }
    double barrier() const noexcept { return barrier_; }
    barrier_type barrier_kind() const noexcept { return kind_; }
    double rebate() const noexcept { return rebate_; }
    ito::rebate_timing rebate_payment() const noexcept { return timing_; }
    observation_mode observation() const noexcept { return observation_; }
    const std::vector<date>& observation_dates() const noexcept { return observations_.dates(); }
    const ObservationSchedule& schedule() const noexcept { return observations_; }
    date expiry() const noexcept { return expiry_; }
    friend bool operator==(const BarrierOption&, const BarrierOption&) = default;

private:
    BarrierOption(option_type type, double strike, date expiry, double barrier, barrier_type kind,
                  double rebate, ito::rebate_timing timing, observation_mode observation,
                  ObservationSchedule observations)
        : type_(type), strike_(strike), barrier_(barrier), kind_(kind), rebate_(rebate), timing_(timing),
          observation_(observation), observations_(std::move(observations)), expiry_(expiry) {}
    option_type type_;
    double strike_;
    double barrier_;
    barrier_type kind_;
    double rebate_;
    ito::rebate_timing timing_;
    observation_mode observation_;
    ObservationSchedule observations_;
    date expiry_;
    friend result<BarrierOption> make_barrier_option(option_type, double, date, double, barrier_type,
                                                     double, ito::rebate_timing, observation_mode, std::vector<date>);
    friend result<BarrierOption> make_barrier_option(
        option_type, double, date, double, barrier_type, double, rebate_timing,
        observation_mode, ObservationSchedule);
};
[[nodiscard]] inline result<BarrierOption> make_barrier_option(
    option_type type, double strike, date expiry, double barrier, barrier_type kind,
    double rebate = 0.0, rebate_timing timing = rebate_timing::at_expiry,
    observation_mode observation = observation_mode::continuous,
    std::vector<date> observations = {})
{
    if (type != option_type::call && type != option_type::put)
        return std::unexpected(Error{error_category::invalid_option, "option type must be call or put"});
    if (!std::isfinite(strike) || strike <= 0.0)
        return std::unexpected(Error{error_category::invalid_strike, "strike must be finite and positive"});
    if (!std::isfinite(barrier) || barrier <= 0.0 || !std::isfinite(rebate) || rebate < 0.0)
        return std::unexpected(Error{error_category::invalid_parameter, "barrier terms must be finite and non-negative"});
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
    }
    if (observation == observation_mode::continuous)
        return BarrierOption{type, strike, expiry, barrier, kind, rebate, timing, observation,
                             *make_observation_schedule({}, expiry, expiry, all_days_calendar())};
    auto schedule = make_observation_schedule(observations, observations.front(), expiry, all_days_calendar());
    if (!schedule)
        return std::unexpected(Error{error_category::invalid_schedule,
                                     "observation dates must be ordered and precede expiry"});
    return BarrierOption{type, strike, expiry, barrier, kind, rebate, timing, observation, std::move(*schedule)};
}

[[nodiscard]] inline result<BarrierOption> make_barrier_option(
    option_type type, double strike, date expiry, double barrier, barrier_type kind,
    double rebate, rebate_timing timing, observation_mode observation, ObservationSchedule schedule)
{
    if (observation == observation_mode::continuous && !schedule.empty())
        return std::unexpected(Error{error_category::invalid_schedule, "continuous barriers cannot have observations"});
    if (observation == observation_mode::scheduled && schedule.empty())
        return std::unexpected(Error{error_category::invalid_schedule, "scheduled barriers require observations"});
    auto base = make_barrier_option(type, strike, expiry, barrier, kind, rebate, timing,
                                    observation, schedule.dates());
    if (!base) return std::unexpected(base.error());
    base->observations_ = std::move(schedule);
    return base;
}

class AnalyticDigitalEngine {
public:
    static constexpr risk_measure_set supported_risk_measures =
        risk_bit(risk_measure::price) | risk_bit(risk_measure::delta) | risk_bit(risk_measure::gamma);

    [[nodiscard]] result<PricingResult> price(const CashOrNothingOption&, const PricingContext&) const;
    [[nodiscard]] result<PricingResult> price(const AssetOrNothingOption&, const PricingContext&) const;
    [[nodiscard]] result<PricingResult> price(
        const CashOrNothingOption&, const PricingContext&, PricingRequest) const;
    [[nodiscard]] result<PricingResult> price(
        const AssetOrNothingOption&, const PricingContext&, PricingRequest) const;
};

class AnalyticBarrierEngine {
public:
    static constexpr risk_measure_set supported_risk_measures = risk_bit(risk_measure::price);

    [[nodiscard]] result<PricingResult> price(const BarrierOption&, const PricingContext&) const;
    [[nodiscard]] result<PricingResult> price(const BarrierOption&, const PricingContext&, PricingRequest) const;
};

} // namespace ito
