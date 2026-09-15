#pragma once

#include <algorithm>
#include <cmath>
#include <optional>
#include <utility>
#include <vector>

#include <kiyosi/instruments/barrier/terms.hpp>
#include <kiyosi/instruments/option_terms.hpp>

namespace kiyosi {

class BinaryBarrierOption;

struct BinaryBarrierTerms {
    std::optional<option_type> type;
    double strike{};
    date effective{};
    date expiry{};
    double barrier{};
    barrier_type barrier_kind{};
    double payout{};
    bool asset_settlement{};
    rebate_timing settlement_timing{rebate_timing::at_expiry};
    observation_mode observation{observation_mode::continuous};
    std::vector<date> observations;
};

[[nodiscard]] result<BinaryBarrierOption> make_binary_barrier_option(BinaryBarrierTerms);

/// Barrier contract paying a fixed amount or the asset; an absent option type is a touch contract.
class BinaryBarrierOption {
public:
    std::optional<option_type> type() const noexcept { return type_; }
    double strike() const noexcept { return strike_; }
    double payout() const noexcept { return payout_; }
    bool asset_settlement() const noexcept { return asset_; }
    rebate_timing settlement_timing() const noexcept { return timing_; }

    const BarrierTerms& barrier_terms() const noexcept { return barrier_; }
    double barrier() const noexcept { return barrier_.barrier(); }
    barrier_type barrier_kind() const noexcept { return barrier_.kind(); }
    observation_mode observation() const noexcept { return barrier_.observation(); }
    const ObservationSchedule& schedule() const noexcept { return barrier_.schedule(); }
    const std::vector<date>& observation_dates() const noexcept { return barrier_.observation_dates(); }
    double observation_interval() const noexcept { return barrier_.observation_interval(); }
    date effective() const noexcept { return barrier_.effective(); }
    date expiry() const noexcept { return barrier_.expiry(); }

    friend bool operator==(const BinaryBarrierOption&, const BinaryBarrierOption&) = default;

private:
    BinaryBarrierOption(std::optional<option_type> type, double strike, double payout, bool asset,
                        rebate_timing timing, BarrierTerms barrier)
        : type_(type), strike_(strike), payout_(payout), asset_(asset), timing_(timing),
          barrier_(std::move(barrier)) {}

    std::optional<option_type> type_;
    double strike_;
    double payout_;
    bool asset_;
    rebate_timing timing_;
    BarrierTerms barrier_;

    friend result<BinaryBarrierOption> make_binary_barrier_option(BinaryBarrierTerms);
};

[[nodiscard]] inline result<BinaryBarrierOption> make_binary_barrier_option(BinaryBarrierTerms contract_terms)
{
    auto [type, strike, effective, expiry, barrier, kind, payout, asset, timing, observation, observations] =
        std::move(contract_terms);
    if (type && *type != option_type::call && *type != option_type::put)
        return std::unexpected(Error{error_category::invalid_option, "option type must be call or put"});
    if (!std::isfinite(strike) || strike <= 0.0 || !std::isfinite(barrier) || barrier <= 0.0 ||
        !std::isfinite(payout) || payout < 0.0 || (!asset && payout == 0.0))
        return std::unexpected(Error{error_category::invalid_parameter, "binary barrier terms are invalid"});
    if (!is_valid_date(effective) || !is_valid_date(expiry) || effective > expiry)
        return std::unexpected(Error{error_category::invalid_schedule,
                                     "binary barrier life dates are invalid"});
    if (kind != barrier_type::up_and_in && kind != barrier_type::up_and_out &&
        kind != barrier_type::down_and_in && kind != barrier_type::down_and_out)
        return std::unexpected(Error{error_category::invalid_option, "invalid barrier type"});
    if (timing != rebate_timing::at_hit && timing != rebate_timing::at_expiry)
        return std::unexpected(Error{error_category::invalid_option, "invalid settlement timing"});
    if (timing == rebate_timing::at_hit && (!is_knock_in_barrier(kind) || type.has_value()))
        return std::unexpected(Error{error_category::invalid_option,
                                     "at-hit settlement requires a knock-in one-touch"});
    if (asset && timing == rebate_timing::at_hit &&
        std::abs(payout - barrier) > 1e-12 * std::max(1.0, std::abs(barrier)))
        return std::unexpected(Error{error_category::invalid_parameter,
                                     "asset at-hit payout must equal barrier"});
    auto terms = detail::make_barrier_terms(barrier, kind, observation, std::move(observations),
                                            effective, expiry);
    if (!terms) return std::unexpected(terms.error());
    return BinaryBarrierOption{type, strike, payout, asset, timing, std::move(*terms)};
}

[[nodiscard]] inline result<BinaryBarrierOption> make_one_touch_up(
    double strike, date effective, date expiry, double barrier, double payout,
    rebate_timing timing = BinaryBarrierTerms{}.settlement_timing)
{
    return make_binary_barrier_option({.type = std::nullopt,
                                       .strike = strike,
                                       .effective = effective,
                                       .expiry = expiry,
                                       .barrier = barrier,
                                       .barrier_kind = barrier_type::up_and_in,
                                       .payout = payout,
                                       .asset_settlement = false,
                                       .settlement_timing = timing});
}

[[nodiscard]] inline result<BinaryBarrierOption> make_one_touch_down(
    double strike, date effective, date expiry, double barrier, double payout,
    rebate_timing timing = BinaryBarrierTerms{}.settlement_timing)
{
    return make_binary_barrier_option({.type = std::nullopt,
                                       .strike = strike,
                                       .effective = effective,
                                       .expiry = expiry,
                                       .barrier = barrier,
                                       .barrier_kind = barrier_type::down_and_in,
                                       .payout = payout,
                                       .asset_settlement = false,
                                       .settlement_timing = timing});
}

[[nodiscard]] inline result<BinaryBarrierOption> make_no_touch_up(
    double strike, date effective, date expiry, double barrier, double payout)
{
    return make_binary_barrier_option({.type = std::nullopt,
                                       .strike = strike,
                                       .effective = effective,
                                       .expiry = expiry,
                                       .barrier = barrier,
                                       .barrier_kind = barrier_type::up_and_out,
                                       .payout = payout,
                                       .asset_settlement = false,
                                       .settlement_timing = rebate_timing::at_expiry});
}

[[nodiscard]] inline result<BinaryBarrierOption> make_no_touch_down(
    double strike, date effective, date expiry, double barrier, double payout)
{
    return make_binary_barrier_option({.type = std::nullopt,
                                       .strike = strike,
                                       .effective = effective,
                                       .expiry = expiry,
                                       .barrier = barrier,
                                       .barrier_kind = barrier_type::down_and_out,
                                       .payout = payout,
                                       .asset_settlement = false,
                                       .settlement_timing = rebate_timing::at_expiry});
}

} // namespace kiyosi
