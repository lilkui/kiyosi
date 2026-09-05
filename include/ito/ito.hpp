#pragma once

#include <chrono>
#include <cmath>
#include <expected>
#include <string>
#include <utility>

namespace ito {

inline constexpr int version_major = 0;
inline constexpr int version_minor = 1;
inline constexpr int version_patch = 0;

using date = std::chrono::sys_days;
using Date = date;

enum class error_category : unsigned char {
    invalid_option = 1,
    invalid_parameter = 2,
    invalid_asset_price = 3,
    invalid_date = 4,
    invalid_expiry = 5,
    invalid_result = 6,
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
    return EuropeanOption{type, strike, expiry};
}

inline result<EuropeanOption> make_european_option(double strike, date expiry, option_type type)
{
    return make_european_option(type, strike, expiry);
}

inline result<void> validate_expiry(date valuation_date, date expiry);

inline result<EuropeanOption> make_european_option(
    option_type type, double strike, date valuation_date, date expiry)
{
    auto valid_expiry = validate_expiry(valuation_date, expiry);
    if (!valid_expiry) return std::unexpected(valid_expiry.error());
    return make_european_option(type, strike, expiry);
}

inline result<void> validate_expiry(date valuation_date, date expiry)
{
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

    friend bool operator==(const PricingContext&, const PricingContext&) = default;

private:
    PricingContext(BsmParameters parameters, AssetPrice asset_price, date valuation_date)
        : parameters_(std::move(parameters)), asset_price_(asset_price), valuation_date_(valuation_date) {}

    BsmParameters parameters_;
    AssetPrice asset_price_;
    date valuation_date_;

    friend result<PricingContext> make_pricing_context(BsmParameters, AssetPrice, date);
};

using pricing_context = PricingContext;

inline result<PricingContext> make_pricing_context(
    BsmParameters parameters, AssetPrice asset_price, date valuation_date)
{
    return PricingContext{std::move(parameters), asset_price, valuation_date};
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
    BsmParameters parameters, AssetPrice asset, date valuation_date, const EuropeanOption& option)
{
    auto expiry = validate_expiry(valuation_date, option.expiry());
    if (!expiry) {
        return std::unexpected(expiry.error());
    }
    return make_pricing_context(std::move(parameters), asset, valuation_date);
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

}
