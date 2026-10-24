#pragma once

#include <kiyosi/instruments/barrier/option.hpp>

namespace kiyosi {

/// Either boundary activates or terminates the European vanilla payoff.
enum class DoubleBarrierType : std::uint8_t { knock_in,
                                              knock_out };

/// Inputs for a fixed-boundary European double-barrier option.
struct DoubleBarrierOptionTerms {
    OptionType option_type{};
    double strike{};
    Date effective_date{};
    Date expiry_date{};
    double lower_barrier{};
    double upper_barrier{};
    DoubleBarrierType barrier_type{};
    double rebate{};
    RebateTiming rebate_timing{RebateTiming::at_expiry};
    ObservationMode observation_mode{ObservationMode::continuous};
    std::vector<Date> observation_dates{};
    std::optional<BarrierTouchState> touch_state{};
};

class DoubleBarrierOption;
class DoubleBarrierTerms;
namespace detail {
[[nodiscard]] Result<DoubleBarrierTerms> make_double_barrier_terms(
    double, double, DoubleBarrierType, ObservationMode, std::vector<Date>, Date, Date,
    std::optional<BarrierTouchState>);
}
/// Validates all terms before constructing an owning option.
[[nodiscard]] Result<DoubleBarrierOption> make_double_barrier_option(DoubleBarrierOptionTerms);

/// Two fixed positive ordered boundaries sharing one monitoring schedule and history.
/// A touch is inclusive at either boundary. History is strictly before valuation.
/// Concurrent reads on shared or distinct instances are safe without concurrent assignment.
class DoubleBarrierTerms {
public:
    double lower_barrier() const noexcept { return lower_.barrier_level(); }
    double upper_barrier() const noexcept { return upper_; }
    DoubleBarrierType barrier_type() const noexcept { return is_knock_in() ? DoubleBarrierType::knock_in : DoubleBarrierType::knock_out; }
    bool is_knock_in() const noexcept { return lower_.is_knock_in(); }
    bool is_continuous() const noexcept { return lower_.is_continuous(); }
    ObservationMode observation_mode() const noexcept { return lower_.observation_mode(); }
    const ObservationSchedule& observation_schedule() const noexcept { return lower_.observation_schedule(); }
    const std::vector<Date>& observation_dates() const noexcept { return lower_.observation_dates(); }
    Date effective_date() const noexcept { return lower_.effective_date(); }
    Date expiry_date() const noexcept { return lower_.expiry_date(); }
    std::optional<BarrierTouchState> touch_state() const noexcept { return lower_.touch_state(); }
    [[nodiscard]] Result<bool> was_touched_before(Timestamp time) const { return lower_.was_touched_before(time); }
    bool is_monitored_on(Date date) const noexcept { return lower_.is_monitored_on(date); }
    bool is_monitored_at(Timestamp time) const noexcept { return lower_.is_monitored_at(time); }
    bool is_breached_by(double spot) const noexcept { return spot <= lower_barrier() || spot >= upper_; }
    friend bool operator==(const DoubleBarrierTerms&, const DoubleBarrierTerms&) = default;

private:
    DoubleBarrierTerms(BarrierTerms lower, double upper) : lower_(std::move(lower)), upper_(upper) {}
    // Copy the allocating member before committing dependent terms.
    BarrierTerms lower_;
    double upper_;
    friend Result<DoubleBarrierTerms> detail::make_double_barrier_terms(
        double, double, DoubleBarrierType, ObservationMode, std::vector<Date>, Date, Date,
        std::optional<BarrierTouchState>);
};

/// European call or put activated/terminated by the first touch of either boundary.
/// Knock-out rebates share one amount and pay at hit or expiry. A never-activated
/// knock-in rebate pays at expiry. Past at-hit payments have no remaining value.
/// Values own their schedules; references into them last until assignment/destruction.
/// Concurrent reads and replace calls on shared/distinct instances are safe without
/// concurrent assignment. Copy assignment preserves the original on allocation failure.
class DoubleBarrierOption {
public:
    OptionType option_type() const noexcept { return option_type_; }
    double strike() const noexcept { return strike_; }
    double rebate() const noexcept { return rebate_; }
    RebateTiming rebate_timing() const noexcept { return rebate_timing_; }
    const DoubleBarrierTerms& barrier_terms() const noexcept { return barrier_; }
    double lower_barrier() const noexcept { return barrier_.lower_barrier(); }
    double upper_barrier() const noexcept { return barrier_.upper_barrier(); }
    DoubleBarrierType barrier_type() const noexcept { return barrier_.barrier_type(); }
    ObservationMode observation_mode() const noexcept { return barrier_.observation_mode(); }
    const ObservationSchedule& observation_schedule() const noexcept { return barrier_.observation_schedule(); }
    const std::vector<Date>& observation_dates() const noexcept { return barrier_.observation_dates(); }
    Date effective_date() const noexcept { return barrier_.effective_date(); }
    Date expiry_date() const noexcept { return barrier_.expiry_date(); }
    std::optional<BarrierTouchState> touch_state() const noexcept { return barrier_.touch_state(); }

    /// Returns a validated owning copy, preserving this value on success or failure.
    /// Unknown history is nullopt; time consistency is checked at pricing.
    [[nodiscard]] Result<DoubleBarrierOption> replace(std::optional<BarrierTouchState> touch_state) const
    {
        return make_double_barrier_option({option_type(), strike(), effective_date(), expiry_date(),
                                           lower_barrier(), upper_barrier(), barrier_type(), rebate(),
                                           rebate_timing(), observation_mode(), observation_dates(), touch_state});
    }
    friend bool operator==(const DoubleBarrierOption&, const DoubleBarrierOption&) = default;

private:
    DoubleBarrierOption(DoubleBarrierTerms barrier, OptionType type, double strike, double rebate, RebateTiming timing)
        : barrier_(std::move(barrier)), option_type_(type), strike_(strike), rebate_(rebate), rebate_timing_(timing) {}
    DoubleBarrierTerms barrier_;
    OptionType option_type_;
    double strike_;
    double rebate_;
    RebateTiming rebate_timing_;
    friend Result<DoubleBarrierOption> make_double_barrier_option(DoubleBarrierOptionTerms);
};

[[nodiscard]] inline Result<DoubleBarrierTerms> detail::make_double_barrier_terms(
    double lower_barrier, double upper_barrier, DoubleBarrierType barrier_type,
    ObservationMode observation_mode, std::vector<Date> observation_dates, Date effective_date,
    Date expiry_date, std::optional<BarrierTouchState> touch_state)
{
    if (!std::isfinite(upper_barrier) || !std::isfinite(lower_barrier) ||
        lower_barrier <= 0.0 || upper_barrier <= lower_barrier)
        return std::unexpected(Error{ErrorCategory::invalid_parameter, "double barriers require finite 0 < lower < upper"});
    if (barrier_type != DoubleBarrierType::knock_in && barrier_type != DoubleBarrierType::knock_out)
        return std::unexpected(Error{ErrorCategory::invalid_option, "invalid double barrier type"});
    auto lower = make_barrier_terms(lower_barrier,
                                    barrier_type == DoubleBarrierType::knock_in ? BarrierType::down_and_in : BarrierType::down_and_out,
                                    observation_mode, std::move(observation_dates), effective_date, expiry_date, touch_state);
    if (!lower) return std::unexpected(lower.error());
    return DoubleBarrierTerms{std::move(*lower), upper_barrier};
}

[[nodiscard]] inline Result<DoubleBarrierOption> make_double_barrier_option(DoubleBarrierOptionTerms terms)
{
    auto barrier = detail::make_double_barrier_terms(terms.lower_barrier, terms.upper_barrier, terms.barrier_type,
                                                     terms.observation_mode, std::move(terms.observation_dates),
                                                     terms.effective_date, terms.expiry_date, terms.touch_state);
    if (!barrier) return std::unexpected(barrier.error());
    const auto payoff = detail::validate_barrier_payoff(terms.option_type, terms.strike, terms.rebate, terms.rebate_timing,
                                                        barrier->is_knock_in());
    if (!payoff) return std::unexpected(payoff.error());
    return DoubleBarrierOption{std::move(*barrier),
                               terms.option_type, terms.strike, terms.rebate, terms.rebate_timing};
}

} // namespace kiyosi
