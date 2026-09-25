#pragma once

#include <algorithm>
#include <cmath>
#include <optional>
#include <utility>
#include <vector>

#include <kiyosi/core/day_count.hpp>
#include <kiyosi/core/error.hpp>
#include <kiyosi/core/time.hpp>
#include <kiyosi/market/schedule.hpp>

namespace kiyosi {

/// Direction and activation behavior of a barrier.
enum class BarrierType {
    up_and_in,   ///< Activates when spot reaches or exceeds the barrier.
    up_and_out,  ///< Terminates when spot reaches or exceeds the barrier.
    down_and_in, ///< Activates when spot reaches or falls below the barrier.
    down_and_out ///< Terminates when spot reaches or falls below the barrier.
};

/// Barrier monitoring frequency.
enum class ObservationMode {
    continuous, ///< Monitor continuously throughout the contract life.
    scheduled   ///< Monitor only on explicit observation dates.
};

/// Barrier observations strictly before the valuation time.
enum class BarrierTouchState {
    untouched, ///< No prior observation breached the barrier.
    touched    ///< A prior observation breached the barrier.
};

/// Payment timing for a barrier-option rebate.
enum class RebateTiming {
    at_hit,   ///< Pay when the barrier is hit.
    at_expiry ///< Pay at contract expiry.
};

/// Settlement timing for a touch option.
enum class SettlementTiming {
    at_hit,   ///< Settle when the barrier is hit.
    at_expiry ///< Settle at contract expiry.
};

/// Tests whether a barrier is triggered by upward spot movement.
/// @return `true` for up-and-in and up-and-out barriers.
[[nodiscard]] constexpr bool is_up_barrier(BarrierType kind) noexcept
{
    return kind == BarrierType::up_and_in || kind == BarrierType::up_and_out;
}

/// Tests whether a barrier activates rather than terminates the contract.
/// @return `true` for up-and-in and down-and-in barriers.
[[nodiscard]] constexpr bool is_knock_in_barrier(BarrierType kind) noexcept
{
    return kind == BarrierType::up_and_in || kind == BarrierType::down_and_in;
}

class BarrierTerms;

namespace detail {
[[nodiscard]] Result<BarrierTerms> make_barrier_terms(
    double, BarrierType, ObservationMode, std::vector<Date>, Date, Date, std::optional<BarrierTouchState>);
}

/// Trigger level, knock direction, and monitoring schedule shared by every barrier contract.
class BarrierTerms {
public:
    /// Returns the positive barrier level.
    double barrier_level() const noexcept { return barrier_level_; }
    /// Returns the barrier direction and activation behavior.
    BarrierType barrier_type() const noexcept { return barrier_type_; }
    /// Returns the monitoring frequency.
    kiyosi::ObservationMode observation_mode() const noexcept { return observation_mode_; }
    /// Returns the validated monitoring schedule.
    const ObservationSchedule& observation_schedule() const noexcept { return observation_dates_; }
    /// Returns the ordered monitoring dates; empty for continuous monitoring.
    const std::vector<Date>& observation_dates() const noexcept { return observation_dates_.dates(); }
    /// Returns the first date of the contract life.
    Date effective_date() const noexcept { return effective_date_; }
    /// Returns the final date of the contract life.
    Date expiry_date() const noexcept { return expiry_date_; }
    /// Returns the caller-supplied state before valuation, if supplied.
    std::optional<BarrierTouchState> touch_state() const noexcept { return touch_state_; }

    /// Resolves prior history, rejecting missing or impossible history.
    [[nodiscard]] Result<bool> was_touched_before(Timestamp valuation_time) const
    {
        const bool had_observation = is_continuous()
                                         ? valuation_time > start_of_day(effective_date_)
                                         : std::any_of(observation_dates_.dates().begin(), observation_dates_.dates().end(),
                                                       [&](Date date) { return start_of_day(date) < valuation_time; });
        if (!had_observation && touch_state_ == BarrierTouchState::touched)
            return std::unexpected(Error{ErrorCategory::invalid_option,
                                         "barrier cannot have been touched before monitoring began"});
        if (had_observation && !touch_state_)
            return std::unexpected(Error{ErrorCategory::invalid_parameter,
                                         "prior barrier touch state is required at valuation"});
        return touch_state_ == BarrierTouchState::touched;
    }

    /// Returns whether this is an upward barrier.
    bool is_up() const noexcept { return is_up_barrier(barrier_type_); }
    /// Returns whether this is a knock-in barrier.
    bool is_knock_in() const noexcept { return is_knock_in_barrier(barrier_type_); }
    /// Returns whether monitoring is continuous.
    bool is_continuous() const noexcept { return observation_mode_ == ObservationMode::continuous; }

    /// Mean spacing between monitoring dates, used for the BGK discrete-barrier shift.
    double mean_observation_year_fraction() const noexcept
    {
        return is_continuous() || observation_dates_.empty()
                   ? 0.0
                   : *year_fraction(effective_date_, observation_dates_.dates().back()) /
                         static_cast<double>(observation_dates_.size());
    }

    /// Returns whether a scheduled observation can still occur at valuation or later.
    bool has_remaining_observation(Timestamp valuation_time) const noexcept
    {
        return !observation_dates_.empty() &&
               start_of_day(observation_dates_.dates().back()) >= valuation_time;
    }

    /// Tests whether the barrier is monitored on a date.
    /// @param date Date to test.
    /// @return `true` within the contract life for continuous monitoring, or on a scheduled observation date.
    bool is_monitored_on(Date date) const noexcept
    {
        if (date < effective_date_ || date > expiry_date_) return false;
        if (is_continuous()) return true;
        for (const Date event : observation_dates_.dates())
            if (event == date) return true;
        return false;
    }

    /// Tests whether a spot lies on the triggered side of the barrier.
    /// @param spot Spot value to test.
    /// @return `true` when `spot` breaches the barrier.
    bool is_breached_by(double spot) const noexcept
    {
        return is_up() ? spot >= barrier_level_ : spot <= barrier_level_;
    }

    /// Compares the barrier level, type, schedule, life, and touch history.
    friend bool operator==(const BarrierTerms&, const BarrierTerms&) = default;

private:
    BarrierTerms(double barrier_level, BarrierType barrier_type, kiyosi::ObservationMode observation_mode,
                 ObservationSchedule observation_dates, Date effective_date, Date expiry_date,
                 std::optional<BarrierTouchState> touch_state)
        : barrier_level_(barrier_level), barrier_type_(barrier_type), observation_mode_(observation_mode),
          observation_dates_(std::move(observation_dates)), effective_date_(effective_date), expiry_date_(expiry_date),
          touch_state_(touch_state) {}

    double barrier_level_;
    BarrierType barrier_type_;
    kiyosi::ObservationMode observation_mode_;
    ObservationSchedule observation_dates_;
    Date effective_date_;
    Date expiry_date_;
    std::optional<BarrierTouchState> touch_state_;

    friend Result<BarrierTerms> detail::make_barrier_terms(
        double, BarrierType, kiyosi::ObservationMode, std::vector<Date>, Date, Date,
        std::optional<BarrierTouchState>);
};

[[nodiscard]] inline Result<BarrierTerms> detail::make_barrier_terms(
    double barrier_level, BarrierType barrier_type, kiyosi::ObservationMode observation_mode,
    std::vector<Date> observation_dates,
    Date effective_date, Date expiry_date, std::optional<BarrierTouchState> touch_state)
{
    if (!std::isfinite(barrier_level) || barrier_level <= 0.0)
        return std::unexpected(Error{ErrorCategory::invalid_parameter,
                                     "barrier terms must be finite and non-negative"});
    auto life = validate_instrument_life(effective_date, expiry_date);
    if (!life) return std::unexpected(life.error());
    if (barrier_type != BarrierType::up_and_in && barrier_type != BarrierType::up_and_out &&
        barrier_type != BarrierType::down_and_in && barrier_type != BarrierType::down_and_out)
        return std::unexpected(Error{ErrorCategory::invalid_option, "invalid barrier type"});
    if (observation_mode != kiyosi::ObservationMode::continuous &&
        observation_mode != kiyosi::ObservationMode::scheduled)
        return std::unexpected(Error{ErrorCategory::invalid_schedule, "invalid observation mode"});
    if (observation_mode == kiyosi::ObservationMode::continuous && !observation_dates.empty())
        return std::unexpected(Error{ErrorCategory::invalid_schedule,
                                     "continuous barriers cannot have observation dates"});
    if (observation_mode == kiyosi::ObservationMode::scheduled && observation_dates.empty())
        return std::unexpected(Error{ErrorCategory::invalid_schedule,
                                     "scheduled barriers require observation dates"});
    if (touch_state && *touch_state != BarrierTouchState::untouched &&
        *touch_state != BarrierTouchState::touched)
        return std::unexpected(Error{ErrorCategory::invalid_option, "invalid barrier touch state"});
    auto schedule = detail::make_date_schedule(std::move(observation_dates), effective_date, expiry_date);
    if (!schedule) return std::unexpected(schedule.error());
    return BarrierTerms{barrier_level, barrier_type, observation_mode, std::move(*schedule), effective_date, expiry_date,
                        touch_state};
}

} // namespace kiyosi
