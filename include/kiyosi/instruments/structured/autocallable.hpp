#pragma once

#include <algorithm>
#include <cmath>
#include <optional>
#include <utility>
#include <vector>

#include <kiyosi/core/error.hpp>
#include <kiyosi/core/time.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/market/schedule.hpp>

namespace kiyosi {

/// Monitoring frequency for an autocallable knock-in barrier.
enum class KnockInObservationMode {
    every_trading_day, ///< Observe on every trading day in the contract life.
    at_expiry          ///< Observe only at expiry.
};

/// Barrier events known before valuation.
enum class AutocallableBarrierState {
    none,        ///< No barrier event has occurred.
    knocked_out, ///< The note has already knocked out.
    knocked_in   ///< The downside barrier has already been breached.
};

/// Principal, knock-out ladder, and settlement strikes shared by every autocallable structure.
/// Valuation excludes cashflows settled before the valuation time.
class AutocallableNote {
public:
    /// Returns the reference spot used to normalize contract levels.
    double initial_spot() const noexcept { return terms_.initial_spot; }
    /// Returns one positive knock-out level per observation date.
    const std::vector<double>& knock_out_levels() const noexcept { return terms_.knock_out_levels; }
    /// Returns the upper settlement strike.
    double upper_strike() const noexcept { return terms_.upper_strike; }
    /// Returns the lower settlement strike.
    double lower_strike() const noexcept { return terms_.lower_strike; }
    /// Returns the strictly ordered knock-out observation dates.
    const std::vector<Date>& observation_dates() const noexcept { return terms_.observation_dates; }
    /// Returns the non-negative principal multiplier.
    double principal_ratio() const noexcept { return terms_.principal_ratio; }
    /// Returns the first date of the note life.
    Date effective_date() const noexcept { return terms_.effective_date; }
    /// Returns the final date of the note life.
    Date expiry_date() const noexcept { return terms_.expiry_date; }
    /// Returns the supplied state before valuation, or nullopt when history is unknown.
    std::optional<AutocallableBarrierState> barrier_state() const noexcept { return terms_.barrier_state; }
    /// Compares all shared autocallable terms.
    friend bool operator==(const AutocallableNote&, const AutocallableNote&) = default;

private:
    AutocallableNote(double initial_spot, std::vector<double> knock_out_levels, double upper_strike,
                     double lower_strike, std::vector<Date> observation_dates, double principal_ratio,
                     std::optional<AutocallableBarrierState> barrier_state, Date effective_date, Date expiry_date)
        : terms_{initial_spot, std::move(knock_out_levels), upper_strike, lower_strike,
                 std::move(observation_dates), principal_ratio, barrier_state, effective_date, expiry_date} {}

    struct Terms {
        double initial_spot;
        std::vector<double> knock_out_levels;
        double upper_strike;
        double lower_strike;
        std::vector<Date> observation_dates;
        double principal_ratio;
        std::optional<AutocallableBarrierState> barrier_state;
        Date effective_date;
        Date expiry_date;
        friend bool operator==(const Terms&, const Terms&) = default;
    } terms_;

    friend class KnockInAutocallableNote;
    friend class BinarySnowballOption;
};

/// An autocallable note carrying a downside knock-in barrier.
class KnockInAutocallableNote {
public:
    /// Returns the reference spot used to normalize contract levels.
    double initial_spot() const noexcept { return note_.initial_spot(); }
    /// Returns one positive knock-out level per observation date.
    const std::vector<double>& knock_out_levels() const noexcept { return note_.knock_out_levels(); }
    /// Returns the upper settlement strike.
    double upper_strike() const noexcept { return note_.upper_strike(); }
    /// Returns the lower settlement strike.
    double lower_strike() const noexcept { return note_.lower_strike(); }
    /// Returns the strictly ordered knock-out observation dates.
    const std::vector<Date>& observation_dates() const noexcept { return note_.observation_dates(); }
    /// Returns the non-negative principal multiplier.
    double principal_ratio() const noexcept { return note_.principal_ratio(); }
    /// Returns the first date of the note life.
    Date effective_date() const noexcept { return note_.effective_date(); }
    /// Returns the final date of the note life.
    Date expiry_date() const noexcept { return note_.expiry_date(); }
    /// Returns the supplied state before valuation, or nullopt when history is unknown.
    std::optional<AutocallableBarrierState> barrier_state() const noexcept { return note_.barrier_state(); }
    /// Returns the positive downside knock-in level.
    double knock_in_level() const noexcept { return knock_in_level_; }
    /// Returns the knock-in monitoring frequency.
    KnockInObservationMode knock_in_observation_mode() const noexcept { return knock_in_observation_mode_; }
    /// Compares all shared terms and knock-in terms.
    friend bool operator==(const KnockInAutocallableNote&, const KnockInAutocallableNote&) = default;

private:
    KnockInAutocallableNote(double initial_spot, double knock_in_level, std::vector<double> knock_out_levels,
                            double upper_strike, double lower_strike, std::vector<Date> observation_dates,
                            KnockInObservationMode knock_in_observation_mode, std::optional<AutocallableBarrierState> barrier_state,
                            double principal_ratio, Date effective_date, Date expiry_date)
        : note_(initial_spot, std::move(knock_out_levels), upper_strike, lower_strike,
                std::move(observation_dates), principal_ratio, barrier_state, effective_date, expiry_date),
          knock_in_level_(knock_in_level), knock_in_observation_mode_(knock_in_observation_mode) {}

    AutocallableNote note_;
    double knock_in_level_;
    KnockInObservationMode knock_in_observation_mode_;

    friend class PhoenixOption;
    friend class SnowballOption;
    friend class TernarySnowballOption;
};

/// Authoritative domain validation for every autocallable product; optional features are
/// detected structurally so each product only pays for the checks it needs.
/// @tparam Note Autocallable note exposing the required term accessors.
/// @return Success, or an input-validation error with a stable category.
template <typename Note>
[[nodiscard]] inline Result<void> validate_autocallable_note(const Note& note)
{
    if (!std::isfinite(note.principal_ratio()) || note.principal_ratio() < 0.0)
        return std::unexpected(Error{ErrorCategory::invalid_parameter, "autocallable terms are invalid"});
    if constexpr (requires { note.initial_spot(); note.upper_strike(); note.lower_strike(); })
        if (!std::isfinite(note.initial_spot()) || note.initial_spot() <= 0.0 ||
            !std::isfinite(note.upper_strike()) || note.upper_strike() <= 0.0 ||
            !std::isfinite(note.lower_strike()) || note.lower_strike() < 0.0 ||
            note.lower_strike() > note.upper_strike())
            return std::unexpected(Error{ErrorCategory::invalid_parameter, "autocallable terms are invalid"});
    auto life = validate_instrument_life(note.effective_date(), note.expiry_date());
    if (!life) return std::unexpected(life.error());
    if (note.observation_dates().empty() ||
        note.knock_out_levels().size() != note.observation_dates().size())
        return std::unexpected(Error{ErrorCategory::invalid_schedule, "autocallable schedule is invalid"});
    auto schedule = validate_date_schedule(note.observation_dates(), note.effective_date(), note.expiry_date());
    if (!schedule) return std::unexpected(schedule.error());
    for (std::size_t index = 0; index < note.observation_dates().size(); ++index) {
        if (!std::isfinite(note.knock_out_levels()[index]) || note.knock_out_levels()[index] <= 0.0)
            return std::unexpected(Error{ErrorCategory::invalid_parameter, "knock-out levels are invalid"});
    }
    if (note.barrier_state() && note.barrier_state() != AutocallableBarrierState::none &&
        note.barrier_state() != AutocallableBarrierState::knocked_out &&
        note.barrier_state() != AutocallableBarrierState::knocked_in)
        return std::unexpected(Error{ErrorCategory::invalid_parameter,
                                     "autocallable touch status is invalid"});
    if constexpr (requires { note.knock_in_level(); }) {
        if (!std::isfinite(note.knock_in_level()) || note.knock_in_level() <= 0.0 ||
            (note.knock_in_observation_mode() != KnockInObservationMode::every_trading_day &&
             note.knock_in_observation_mode() != KnockInObservationMode::at_expiry))
            return std::unexpected(Error{ErrorCategory::invalid_parameter, "knock-in terms are invalid"});
    } else if (note.barrier_state() == AutocallableBarrierState::knocked_in) {
        return std::unexpected(Error{ErrorCategory::invalid_parameter,
                                     "knock-in state requires a knock-in barrier"});
    }
    if constexpr (requires { note.coupon_rate(); }) {
        if (!std::isfinite(note.coupon_rate()))
            return std::unexpected(Error{ErrorCategory::invalid_parameter, "Phoenix coupon is invalid"});
        if (note.coupon_barrier_levels().size() != note.observation_dates().size())
            return std::unexpected(Error{ErrorCategory::invalid_schedule,
                                         "Phoenix coupon schedule is invalid"});
        for (double barrier : note.coupon_barrier_levels())
            if (!std::isfinite(barrier) || barrier < 0.0)
                return std::unexpected(Error{ErrorCategory::invalid_parameter,
                                             "coupon barrier levels are invalid"});
    }
    if constexpr (requires { note.knock_out_coupon_rates(); }) {
        if (note.knock_out_coupon_rates().size() != note.observation_dates().size())
            return std::unexpected(Error{ErrorCategory::invalid_schedule,
                                         "coupon schedule count is invalid"});
        for (double coupon : note.knock_out_coupon_rates())
            if (!std::isfinite(coupon))
                return std::unexpected(Error{ErrorCategory::invalid_parameter, "coupon rates are invalid"});
        if (!std::isfinite(note.maturity_coupon_rate()))
            return std::unexpected(Error{ErrorCategory::invalid_parameter, "maturity coupon is invalid"});
    }
    if constexpr (requires { note.minimum_coupon_rate(); }) {
        if (!std::isfinite(note.minimum_coupon_rate()) ||
            note.minimum_coupon_rate() > note.maturity_coupon_rate())
            return std::unexpected(Error{ErrorCategory::invalid_parameter, "minimum coupon is invalid"});
    }
    return {};
}

/// Validates the caller's history snapshot against events before valuation.
template <typename Note>
[[nodiscard]] inline Result<void> validate_autocallable_history(
    const Note& note, const PricingContext& context)
{
    const Timestamp valuation = context.valuation_time();
    const bool had_knock_out_observation = std::any_of(
        note.observation_dates().begin(), note.observation_dates().end(),
        [&](Date date) { return start_of_day(date) < valuation; });
    bool had_knock_in_observation = false;
    if constexpr (requires { note.knock_in_observation_mode(); })
        if (note.knock_in_observation_mode() == KnockInObservationMode::every_trading_day)
            for (Date date = note.effective_date(); start_of_day(date) < valuation;
                 date += std::chrono::days{1})
                if (context.calendar().is_trading_day(date)) {
                    had_knock_in_observation = true;
                    break;
                }
    if (!note.barrier_state() && (had_knock_out_observation || had_knock_in_observation))
        return std::unexpected(Error{ErrorCategory::invalid_parameter,
                                     "prior autocallable barrier state is required at valuation"});
    if (note.barrier_state() == AutocallableBarrierState::knocked_out && !had_knock_out_observation)
        return std::unexpected(Error{ErrorCategory::invalid_option,
                                     "autocallable cannot have knocked out before its first observation"});
    if (note.barrier_state() == AutocallableBarrierState::knocked_in && !had_knock_in_observation)
        return std::unexpected(Error{ErrorCategory::invalid_option,
                                     "autocallable cannot have knocked in before monitoring began"});
    return {};
}

namespace detail {

template <typename Note>
[[nodiscard]] inline Result<Note> validate_and_return_autocallable_note(Note note)
{
    auto valid = validate_autocallable_note(note);
    if (!valid) return std::unexpected(valid.error());
    return std::move(note);
}

} // namespace detail

} // namespace kiyosi
