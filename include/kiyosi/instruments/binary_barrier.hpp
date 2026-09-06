#pragma once

#include <optional>
#include <kiyosi/instruments/barrier.hpp>

namespace kiyosi {
class BinaryBarrierOption {
public:
    std::optional<option_type> type() const noexcept { return type_; }
    double strike() const noexcept { return strike_; }
    double barrier() const noexcept { return barrier_; }
    barrier_type barrier_kind() const noexcept { return kind_; }
    double payout() const noexcept { return payout_; }
    bool asset_settlement() const noexcept { return asset_; }
    date expiry() const noexcept { return expiry_; }
    friend bool operator==(const BinaryBarrierOption&, const BinaryBarrierOption&) = default;

private:
    BinaryBarrierOption(std::optional<option_type> type, double strike, date expiry, double barrier,
                        barrier_type kind, double payout, bool asset)
        : type_(type), strike_(strike), expiry_(expiry), barrier_(barrier), kind_(kind), payout_(payout), asset_(asset) {}
    std::optional<option_type> type_;
    double strike_;
    date expiry_;
    double barrier_;
    barrier_type kind_;
    double payout_;
    bool asset_;
    friend result<BinaryBarrierOption> make_binary_barrier_option(std::optional<option_type>, double, date, double, barrier_type, double, bool);
};

[[nodiscard]] inline result<BinaryBarrierOption> make_binary_barrier_option(
    std::optional<option_type> type, double strike, date expiry, double barrier,
    barrier_type kind, double payout, bool asset = false)
{
    if (type && *type != option_type::call && *type != option_type::put)
        return std::unexpected(Error{error_category::invalid_option, "option type must be call or put"});
    if (!std::isfinite(strike) || strike <= 0.0 || !std::isfinite(barrier) || barrier <= 0.0 ||
        !std::isfinite(payout) || payout <= 0.0)
        return std::unexpected(Error{error_category::invalid_parameter, "binary barrier terms are invalid"});
    if (!is_valid_date(expiry)) return std::unexpected(Error{error_category::invalid_date, "expiry must be valid"});
    return BinaryBarrierOption{type, strike, expiry, barrier, kind, payout, asset};
}
using CashOrNothingBarrierOption = BinaryBarrierOption;
using AssetOrNothingBarrierOption = BinaryBarrierOption;

[[nodiscard]] inline result<CashOrNothingBarrierOption> make_cash_or_nothing_barrier_option(
    option_type type, double strike, date expiry, double barrier, barrier_type kind, double payout)
{
    return make_binary_barrier_option(type, strike, expiry, barrier, kind, payout, false);
}
[[nodiscard]] inline result<AssetOrNothingBarrierOption> make_asset_or_nothing_barrier_option(
    option_type type, double strike, date expiry, double barrier, barrier_type kind)
{
    return make_binary_barrier_option(type, strike, expiry, barrier, kind, 1.0, true);
}
[[nodiscard]] inline result<CashOrNothingBarrierOption> make_cash_or_nothing_barrier_option(
    double strike, date expiry, double barrier, barrier_type kind, double payout)
{
    return make_binary_barrier_option(std::nullopt, strike, expiry, barrier, kind, payout, false);
}
} // namespace kiyosi
