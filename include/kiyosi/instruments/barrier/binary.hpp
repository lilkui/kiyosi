#pragma once

#include <utility>
#include <variant>
#include <vector>

#include <kiyosi/instruments/barrier/terms.hpp>
#include <kiyosi/instruments/option_terms.hpp>
#include <kiyosi/instruments/payoff.hpp>

namespace kiyosi {

class BinaryBarrierOption;
class TouchOption;

struct BinaryBarrierTerms {
    option_type type{};
    double strike{};
    date effective{};
    date expiry{};
    double barrier{};
    barrier_type barrier_kind{};
    kiyosi::observation_mode observation_mode{kiyosi::observation_mode::continuous};
    std::vector<date> observation_dates;
};

[[nodiscard]] result<BinaryBarrierOption> make_cash_binary_barrier_option(
    BinaryBarrierTerms, double payout);
[[nodiscard]] result<BinaryBarrierOption> make_asset_binary_barrier_option(BinaryBarrierTerms);

namespace detail {
[[nodiscard]] result<TouchOption> make_touch_option(
    date, date, double, BinaryPayoff, barrier_type, settlement_timing,
    observation_mode, std::vector<date>);
}

/// Strike-based binary option whose payoff also depends on a barrier event.
class BinaryBarrierOption {
public:
    option_type type() const noexcept { return terms_.type(); }
    double strike() const noexcept { return terms_.strike(); }
    const BinaryPayoff& payoff() const noexcept { return payoff_; }
    payoff_type payoff_kind() const noexcept { return kiyosi::payoff_kind(payoff_); }

    const BarrierTerms& barrier_terms() const noexcept { return barrier_; }
    double barrier() const noexcept { return barrier_.barrier(); }
    barrier_type barrier_kind() const noexcept { return barrier_.kind(); }
    kiyosi::observation_mode observation_mode() const noexcept { return barrier_.observation_mode(); }
    const ObservationSchedule& schedule() const noexcept { return barrier_.schedule(); }
    const std::vector<date>& observation_dates() const noexcept { return barrier_.observation_dates(); }
    double observation_interval() const noexcept { return barrier_.observation_interval(); }
    date effective() const noexcept { return terms_.effective(); }
    date expiry() const noexcept { return terms_.expiry(); }

    friend bool operator==(const BinaryBarrierOption&, const BinaryBarrierOption&) = default;

private:
    BinaryBarrierOption(OptionTerms terms, BinaryPayoff payoff, BarrierTerms barrier)
        : terms_(std::move(terms)), payoff_(std::move(payoff)), barrier_(std::move(barrier)) {}

    OptionTerms terms_;
    BinaryPayoff payoff_;
    BarrierTerms barrier_;

    friend result<BinaryBarrierOption> make_cash_binary_barrier_option(
        BinaryBarrierTerms, double);
    friend result<BinaryBarrierOption> make_asset_binary_barrier_option(BinaryBarrierTerms);
};

namespace detail {

[[nodiscard]] inline result<std::pair<OptionTerms, BarrierTerms>> make_binary_barrier_terms(
    BinaryBarrierTerms terms)
{
    auto option = make_option_terms(terms.type, terms.strike, terms.effective, terms.expiry);
    if (!option) return std::unexpected(option.error());
    auto barrier = make_barrier_terms(terms.barrier, terms.barrier_kind, terms.observation_mode,
                                      std::move(terms.observation_dates), terms.effective, terms.expiry);
    if (!barrier) return std::unexpected(barrier.error());
    return std::pair{std::move(*option), std::move(*barrier)};
}

} // namespace detail

[[nodiscard]] inline result<BinaryBarrierOption> make_cash_binary_barrier_option(
    BinaryBarrierTerms terms, double payout)
{
    auto payoff = detail::make_cash_or_nothing_payoff(payout);
    if (!payoff) return std::unexpected(payoff.error());
    auto validated = detail::make_binary_barrier_terms(std::move(terms));
    if (!validated) return std::unexpected(validated.error());
    return BinaryBarrierOption{std::move(validated->first), std::move(*payoff),
                               std::move(validated->second)};
}

[[nodiscard]] inline result<BinaryBarrierOption> make_asset_binary_barrier_option(
    BinaryBarrierTerms terms)
{
    auto validated = detail::make_binary_barrier_terms(std::move(terms));
    if (!validated) return std::unexpected(validated.error());
    return BinaryBarrierOption{std::move(validated->first), AssetOrNothingPayoff{},
                               std::move(validated->second)};
}

/// Strike-free one-touch or no-touch contract paying cash or the asset.
class TouchOption {
public:
    const BinaryPayoff& payoff() const noexcept { return payoff_; }
    payoff_type payoff_kind() const noexcept { return kiyosi::payoff_kind(payoff_); }
    kiyosi::settlement_timing settlement_timing() const noexcept { return settlement_timing_; }

    const BarrierTerms& barrier_terms() const noexcept { return barrier_; }
    double barrier() const noexcept { return barrier_.barrier(); }
    bool is_one_touch() const noexcept { return barrier_.is_knock_in(); }
    bool is_up() const noexcept { return barrier_.is_up(); }
    kiyosi::observation_mode observation_mode() const noexcept { return barrier_.observation_mode(); }
    const ObservationSchedule& schedule() const noexcept { return barrier_.schedule(); }
    const std::vector<date>& observation_dates() const noexcept { return barrier_.observation_dates(); }
    double observation_interval() const noexcept { return barrier_.observation_interval(); }
    date effective() const noexcept { return barrier_.effective(); }
    date expiry() const noexcept { return barrier_.expiry(); }

    friend bool operator==(const TouchOption&, const TouchOption&) = default;

private:
    TouchOption(BinaryPayoff payoff, kiyosi::settlement_timing settlement_timing,
                BarrierTerms barrier)
        : payoff_(std::move(payoff)), settlement_timing_(settlement_timing),
          barrier_(std::move(barrier))
    {
    }

    BinaryPayoff payoff_;
    kiyosi::settlement_timing settlement_timing_;
    BarrierTerms barrier_;

    friend result<TouchOption> detail::make_touch_option(
        date, date, double, BinaryPayoff, barrier_type, kiyosi::settlement_timing,
        kiyosi::observation_mode, std::vector<date>);
};

namespace detail {

[[nodiscard]] inline result<TouchOption> make_touch_option(
    date effective, date expiry, double barrier, BinaryPayoff payoff, barrier_type barrier_kind,
    kiyosi::settlement_timing settlement_timing, kiyosi::observation_mode observation_mode,
    std::vector<date> observation_dates)
{
    if (settlement_timing != kiyosi::settlement_timing::at_hit &&
        settlement_timing != kiyosi::settlement_timing::at_expiry)
        return std::unexpected(Error{error_category::invalid_option, "invalid settlement timing"});
    if (settlement_timing == kiyosi::settlement_timing::at_hit &&
        !is_knock_in_barrier(barrier_kind))
        return std::unexpected(Error{error_category::invalid_option,
                                     "at-hit settlement requires a one-touch"});
    auto terms = make_barrier_terms(barrier, barrier_kind, observation_mode,
                                    std::move(observation_dates), effective, expiry);
    if (!terms) return std::unexpected(terms.error());
    return TouchOption{std::move(payoff), settlement_timing, std::move(*terms)};
}

[[nodiscard]] inline result<TouchOption> make_cash_touch_option(
    date effective, date expiry, double barrier, double payout, barrier_type barrier_kind,
    kiyosi::settlement_timing settlement_timing, kiyosi::observation_mode observation_mode,
    std::vector<date> observation_dates)
{
    auto payoff = make_cash_or_nothing_payoff(payout);
    if (!payoff) return std::unexpected(payoff.error());
    return make_touch_option(effective, expiry, barrier, std::move(*payoff), barrier_kind,
                             settlement_timing, observation_mode, std::move(observation_dates));
}

[[nodiscard]] inline result<TouchOption> make_asset_touch_option(
    date effective, date expiry, double barrier, barrier_type barrier_kind,
    kiyosi::settlement_timing settlement_timing, kiyosi::observation_mode observation_mode,
    std::vector<date> observation_dates)
{
    return make_touch_option(effective, expiry, barrier, AssetOrNothingPayoff{}, barrier_kind,
                             settlement_timing, observation_mode, std::move(observation_dates));
}

} // namespace detail

[[nodiscard]] inline result<TouchOption> make_cash_one_touch_up(
    date effective, date expiry, double barrier, double payout,
    kiyosi::settlement_timing settlement_timing = kiyosi::settlement_timing::at_expiry,
    kiyosi::observation_mode observation_mode = kiyosi::observation_mode::continuous,
    std::vector<date> observation_dates = {})
{
    return detail::make_cash_touch_option(effective, expiry, barrier, payout,
                                          barrier_type::up_and_in, settlement_timing, observation_mode,
                                          std::move(observation_dates));
}

[[nodiscard]] inline result<TouchOption> make_cash_one_touch_down(
    date effective, date expiry, double barrier, double payout,
    kiyosi::settlement_timing settlement_timing = kiyosi::settlement_timing::at_expiry,
    kiyosi::observation_mode observation_mode = kiyosi::observation_mode::continuous,
    std::vector<date> observation_dates = {})
{
    return detail::make_cash_touch_option(effective, expiry, barrier, payout,
                                          barrier_type::down_and_in, settlement_timing, observation_mode,
                                          std::move(observation_dates));
}

[[nodiscard]] inline result<TouchOption> make_cash_no_touch_up(
    date effective, date expiry, double barrier, double payout,
    kiyosi::observation_mode observation_mode = kiyosi::observation_mode::continuous,
    std::vector<date> observation_dates = {})
{
    return detail::make_cash_touch_option(effective, expiry, barrier, payout,
                                          barrier_type::up_and_out, settlement_timing::at_expiry,
                                          observation_mode, std::move(observation_dates));
}

[[nodiscard]] inline result<TouchOption> make_cash_no_touch_down(
    date effective, date expiry, double barrier, double payout,
    kiyosi::observation_mode observation_mode = kiyosi::observation_mode::continuous,
    std::vector<date> observation_dates = {})
{
    return detail::make_cash_touch_option(effective, expiry, barrier, payout,
                                          barrier_type::down_and_out, settlement_timing::at_expiry,
                                          observation_mode, std::move(observation_dates));
}

[[nodiscard]] inline result<TouchOption> make_asset_one_touch_up(
    date effective, date expiry, double barrier,
    kiyosi::settlement_timing settlement_timing = kiyosi::settlement_timing::at_expiry,
    kiyosi::observation_mode observation_mode = kiyosi::observation_mode::continuous,
    std::vector<date> observation_dates = {})
{
    return detail::make_asset_touch_option(effective, expiry, barrier, barrier_type::up_and_in,
                                           settlement_timing, observation_mode,
                                           std::move(observation_dates));
}

[[nodiscard]] inline result<TouchOption> make_asset_one_touch_down(
    date effective, date expiry, double barrier,
    kiyosi::settlement_timing settlement_timing = kiyosi::settlement_timing::at_expiry,
    kiyosi::observation_mode observation_mode = kiyosi::observation_mode::continuous,
    std::vector<date> observation_dates = {})
{
    return detail::make_asset_touch_option(effective, expiry, barrier, barrier_type::down_and_in,
                                           settlement_timing, observation_mode,
                                           std::move(observation_dates));
}

[[nodiscard]] inline result<TouchOption> make_asset_no_touch_up(
    date effective, date expiry, double barrier,
    kiyosi::observation_mode observation_mode = kiyosi::observation_mode::continuous,
    std::vector<date> observation_dates = {})
{
    return detail::make_asset_touch_option(effective, expiry, barrier, barrier_type::up_and_out,
                                           settlement_timing::at_expiry, observation_mode,
                                           std::move(observation_dates));
}

[[nodiscard]] inline result<TouchOption> make_asset_no_touch_down(
    date effective, date expiry, double barrier,
    kiyosi::observation_mode observation_mode = kiyosi::observation_mode::continuous,
    std::vector<date> observation_dates = {})
{
    return detail::make_asset_touch_option(effective, expiry, barrier, barrier_type::down_and_out,
                                           settlement_timing::at_expiry, observation_mode,
                                           std::move(observation_dates));
}

} // namespace kiyosi
