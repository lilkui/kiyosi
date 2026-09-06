#pragma once

#include <kiyosi/core/types.hpp>
#include <kiyosi/instruments/vanilla.hpp>

namespace kiyosi {

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


} // namespace kiyosi
