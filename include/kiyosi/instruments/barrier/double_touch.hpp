#pragma once

#include <kiyosi/instruments/barrier/binary.hpp>
#include <kiyosi/instruments/barrier/double.hpp>

namespace kiyosi {

class DoubleTouchOption;
namespace detail {
[[nodiscard]] Result<DoubleTouchOption> make_cash_double_touch_option(
    Date, Date, double, double, double, DoubleBarrierType, SettlementTiming,
    ObservationMode, std::vector<Date>, std::optional<BarrierTouchState>);
}

/// Fixed cash paid if either inclusive boundary is touched (one-touch), or neither
/// is touched (no-touch). Both boundaries share monitoring and strictly prior history.
/// At-hit history marked touched means already paid; no-touch settles only at expiry.
/// Values own schedules; borrowed references last until destruction, moving from, or assignment.
/// Reads and replace calls on shared/distinct instances are safe concurrently without
/// assignment. Failed construction/replace preserves inputs; copy assignment preserves
/// the target on allocation failure. Allocation failures propagate as std::bad_alloc.
class DoubleTouchOption {
public:
    Date effective_date() const noexcept { return barrier_.effective_date(); }
    Date expiry_date() const noexcept { return barrier_.expiry_date(); }
    double lower_barrier() const noexcept { return barrier_.lower_barrier(); }
    double upper_barrier() const noexcept { return barrier_.upper_barrier(); }
    double payout() const noexcept { return payoff_.payout(); }
    bool is_one_touch() const noexcept { return barrier_.is_knock_in(); }
    SettlementTiming settlement_timing() const noexcept { return settlement_timing_; }
    ObservationMode observation_mode() const noexcept { return barrier_.observation_mode(); }
    const ObservationSchedule& observation_schedule() const noexcept { return barrier_.observation_schedule(); }
    const std::vector<Date>& observation_dates() const noexcept { return barrier_.observation_dates(); }
    std::optional<BarrierTouchState> touch_state() const noexcept { return barrier_.touch_state(); }
    const DoubleBarrierTerms& barrier_terms() const noexcept { return barrier_; }

    /// Validated owning copy with history replaced; nullopt means unknown, not untouched.
    /// Pricing validates the history against valuation time. This value is unchanged on failure.
    [[nodiscard]] Result<DoubleTouchOption> replace(std::optional<BarrierTouchState> touch_state) const
    {
        auto barrier = detail::make_double_barrier_terms(lower_barrier(), upper_barrier(), barrier_.barrier_type(),
                                                         observation_mode(), observation_dates(), effective_date(), expiry_date(), touch_state);
        if (!barrier) return std::unexpected(barrier.error());
        return DoubleTouchOption{std::move(*barrier), payoff_, settlement_timing_};
    }
    friend bool operator==(const DoubleTouchOption&, const DoubleTouchOption&) = default;

private:
    DoubleTouchOption(DoubleBarrierTerms barrier, CashOrNothingPayoff payoff, SettlementTiming timing)
        : barrier_(std::move(barrier)), payoff_(payoff), settlement_timing_(timing) {}
    DoubleBarrierTerms barrier_;
    CashOrNothingPayoff payoff_;
    SettlementTiming settlement_timing_;
    friend Result<DoubleTouchOption> detail::make_cash_double_touch_option(
        Date, Date, double, double, double, DoubleBarrierType, SettlementTiming,
        ObservationMode, std::vector<Date>, std::optional<BarrierTouchState>);
};

[[nodiscard]] inline Result<DoubleTouchOption> detail::make_cash_double_touch_option(
    Date effective_date, Date expiry_date, double lower_barrier, double upper_barrier, double payout,
    DoubleBarrierType kind, SettlementTiming timing, ObservationMode mode, std::vector<Date> dates,
    std::optional<BarrierTouchState> history)
{
    const auto payoff = make_cash_or_nothing_payoff(payout);
    if (!payoff) return std::unexpected(payoff.error());
    if ((timing != SettlementTiming::at_hit && timing != SettlementTiming::at_expiry) ||
        (kind == DoubleBarrierType::knock_out && timing != SettlementTiming::at_expiry))
        return std::unexpected(Error{ErrorCategory::invalid_option, "invalid double-touch settlement timing"});
    auto barrier = make_double_barrier_terms(lower_barrier, upper_barrier, kind, mode, std::move(dates),
                                             effective_date, expiry_date, history);
    if (!barrier) return std::unexpected(barrier.error());
    return DoubleTouchOption{std::move(*barrier), *payoff, timing};
}

/// Cash one-touch: the first touch of either boundary activates one payment.
[[nodiscard]] inline Result<DoubleTouchOption> make_cash_double_touch(
    Date effective_date, Date expiry_date, double lower_barrier, double upper_barrier, double payout,
    SettlementTiming settlement_timing = default_one_touch_settlement_timing,
    ObservationMode observation_mode = default_touch_observation_mode,
    std::vector<Date> observation_dates = {}, std::optional<BarrierTouchState> touch_state = std::nullopt)
{
    return detail::make_cash_double_touch_option(effective_date, expiry_date, lower_barrier, upper_barrier, payout,
                                                 DoubleBarrierType::knock_in, settlement_timing, observation_mode, std::move(observation_dates), touch_state);
}

/// Cash no-touch: pays at expiry only if neither boundary was touched during monitoring.
[[nodiscard]] inline Result<DoubleTouchOption> make_cash_double_no_touch(
    Date effective_date, Date expiry_date, double lower_barrier, double upper_barrier, double payout,
    ObservationMode observation_mode = default_touch_observation_mode,
    std::vector<Date> observation_dates = {}, std::optional<BarrierTouchState> touch_state = std::nullopt)
{
    return detail::make_cash_double_touch_option(effective_date, expiry_date, lower_barrier, upper_barrier, payout,
                                                 DoubleBarrierType::knock_out, SettlementTiming::at_expiry, observation_mode, std::move(observation_dates), touch_state);
}

} // namespace kiyosi
