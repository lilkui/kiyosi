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
    rebate_timing settlement_timing() const noexcept { return timing_; }
    observation_mode observation() const noexcept { return observation_; }
    const std::vector<date>& observation_dates() const noexcept { return observations_; }
    date expiry() const noexcept { return expiry_; }
    friend bool operator==(const BinaryBarrierOption&, const BinaryBarrierOption&) = default;

private:
    BinaryBarrierOption(std::optional<option_type> type, double strike, date expiry, double barrier,
                        barrier_type kind, double payout, bool asset, rebate_timing timing,
                        observation_mode observation, std::vector<date> observations)
        : type_(type), strike_(strike), expiry_(expiry), barrier_(barrier), kind_(kind), payout_(payout), asset_(asset),
          timing_(timing), observation_(observation), observations_(std::move(observations)) {}
    std::optional<option_type> type_;
    double strike_;
    date expiry_;
    double barrier_;
    barrier_type kind_;
    double payout_;
    bool asset_;
    rebate_timing timing_;
    observation_mode observation_;
    std::vector<date> observations_;
    friend result<BinaryBarrierOption> make_binary_barrier_option(std::optional<option_type>, double, date, double, barrier_type, double, bool, rebate_timing, observation_mode, std::vector<date>);
};

[[nodiscard]] inline result<BinaryBarrierOption> make_binary_barrier_option(
    std::optional<option_type> type, double strike, date expiry, double barrier,
    barrier_type kind, double payout, bool asset = false,
    rebate_timing timing = rebate_timing::at_expiry,
    observation_mode observation = observation_mode::continuous,
    std::vector<date> observations = {})
{
    if (type && *type != option_type::call && *type != option_type::put)
        return std::unexpected(Error{error_category::invalid_option, "option type must be call or put"});
    if (!std::isfinite(strike) || strike <= 0.0 || !std::isfinite(barrier) || barrier <= 0.0 ||
        !std::isfinite(payout) || payout <= 0.0)
        return std::unexpected(Error{error_category::invalid_parameter, "binary barrier terms are invalid"});
    if (!is_valid_date(expiry)) return std::unexpected(Error{error_category::invalid_date, "expiry must be valid"});
    if (kind != barrier_type::up_and_in && kind != barrier_type::up_and_out &&
        kind != barrier_type::down_and_in && kind != barrier_type::down_and_out)
        return std::unexpected(Error{error_category::invalid_option, "invalid barrier type"});
    if (timing != rebate_timing::at_hit && timing != rebate_timing::at_expiry)
        return std::unexpected(Error{error_category::invalid_option, "invalid settlement timing"});
    if (observation != observation_mode::continuous && observation != observation_mode::scheduled)
        return std::unexpected(Error{error_category::invalid_schedule, "invalid observation mode"});
    if (observation == observation_mode::continuous && !observations.empty())
        return std::unexpected(Error{error_category::invalid_schedule, "continuous barriers cannot have observations"});
    if (observation == observation_mode::scheduled && observations.empty())
        return std::unexpected(Error{error_category::invalid_schedule, "scheduled barriers require observations"});
    for (std::size_t index = 0; index < observations.size(); ++index) {
        if (!is_valid_date(observations[index]) || observations[index] > expiry ||
            (index > 0 && observations[index] <= observations[index - 1]))
            return std::unexpected(Error{error_category::invalid_schedule, "observation dates are invalid"});
    }
    return BinaryBarrierOption{type, strike, expiry, barrier, kind, payout, asset, timing, observation, std::move(observations)};
}
using CashOrNothingBarrierOption = BinaryBarrierOption;
using AssetOrNothingBarrierOption = BinaryBarrierOption;

[[nodiscard]] inline result<CashOrNothingBarrierOption> make_cash_or_nothing_barrier_option(
    option_type type, double strike, date expiry, double barrier, barrier_type kind, double payout,
    rebate_timing timing = rebate_timing::at_expiry, observation_mode observation = observation_mode::continuous,
    std::vector<date> observations = {})
{
    return make_binary_barrier_option(type, strike, expiry, barrier, kind, payout, false, timing, observation,
                                      std::move(observations));
}
[[nodiscard]] inline result<AssetOrNothingBarrierOption> make_asset_or_nothing_barrier_option(
    option_type type, double strike, date expiry, double barrier, barrier_type kind,
    double payout = 1.0, rebate_timing timing = rebate_timing::at_expiry,
    observation_mode observation = observation_mode::continuous,
    std::vector<date> observations = {})
{
    return make_binary_barrier_option(type, strike, expiry, barrier, kind, payout, true, timing, observation,
                                      std::move(observations));
}
[[nodiscard]] inline result<AssetOrNothingBarrierOption> make_asset_or_nothing_barrier_option(
    option_type type, double strike, date expiry, double barrier, barrier_type kind,
    rebate_timing timing, observation_mode observation = observation_mode::continuous,
    std::vector<date> observations = {})
{
    return make_asset_or_nothing_barrier_option(type, strike, expiry, barrier, kind, 1.0, timing, observation,
                                                std::move(observations));
}
[[nodiscard]] inline result<CashOrNothingBarrierOption> make_cash_or_nothing_barrier_option(
    double strike, date expiry, double barrier, barrier_type kind, double payout,
    rebate_timing timing = rebate_timing::at_expiry, observation_mode observation = observation_mode::continuous,
    std::vector<date> observations = {})
{
    return make_binary_barrier_option(std::nullopt, strike, expiry, barrier, kind, payout, false, timing, observation,
                                      std::move(observations));
}
} // namespace kiyosi
