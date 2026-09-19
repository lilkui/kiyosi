#pragma once

#include <cmath>
#include <utility>
#include <vector>

#include <kiyosi/core/error.hpp>
#include <kiyosi/core/time.hpp>
#include <kiyosi/market/schedule.hpp>

namespace kiyosi {

enum class KnockInObservationMode { every_trading_day,
                                    at_expiry };

enum class BarrierTouchStatus { none,
                                  up,
                                  down };

/// Principal, knock-out ladder, and settlement strikes shared by every autocallable structure.
class AutocallableNote {
public:
    double initial_spot() const noexcept { return terms_.initial_spot; }
    const std::vector<double>& knock_out_levels() const noexcept { return terms_.knock_out_levels; }
    double upper_strike() const noexcept { return terms_.upper_strike; }
    double lower_strike() const noexcept { return terms_.lower_strike; }
    const std::vector<Date>& observation_dates() const noexcept { return terms_.observation_dates; }
    double principal_ratio() const noexcept { return terms_.principal_ratio; }
    Date effective_date() const noexcept { return terms_.effective_date; }
    Date expiry_date() const noexcept { return terms_.expiry_date; }
    BarrierTouchStatus touch_status() const noexcept { return terms_.touch_status; }
    friend bool operator==(const AutocallableNote&, const AutocallableNote&) = default;

private:
    AutocallableNote(double initial_spot, std::vector<double> knock_out_levels, double upper_strike,
                     double lower_strike, std::vector<Date> observation_dates, double principal_ratio,
                     BarrierTouchStatus touch_status, Date effective_date, Date expiry_date)
        : terms_{initial_spot, std::move(knock_out_levels), upper_strike, lower_strike,
                 std::move(observation_dates), principal_ratio, touch_status, effective_date, expiry_date} {}

    struct Terms {
        double initial_spot;
        std::vector<double> knock_out_levels;
        double upper_strike;
        double lower_strike;
        std::vector<Date> observation_dates;
        double principal_ratio;
        BarrierTouchStatus touch_status;
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
    double initial_spot() const noexcept { return note_.initial_spot(); }
    const std::vector<double>& knock_out_levels() const noexcept { return note_.knock_out_levels(); }
    double upper_strike() const noexcept { return note_.upper_strike(); }
    double lower_strike() const noexcept { return note_.lower_strike(); }
    const std::vector<Date>& observation_dates() const noexcept { return note_.observation_dates(); }
    double principal_ratio() const noexcept { return note_.principal_ratio(); }
    Date effective_date() const noexcept { return note_.effective_date(); }
    Date expiry_date() const noexcept { return note_.expiry_date(); }
    BarrierTouchStatus touch_status() const noexcept { return note_.touch_status(); }
    double knock_in_level() const noexcept { return knock_in_level_; }
    KnockInObservationMode knock_in_observation_mode() const noexcept { return knock_in_observation_mode_; }
    friend bool operator==(const KnockInAutocallableNote&, const KnockInAutocallableNote&) = default;

private:
    KnockInAutocallableNote(double initial_spot, double knock_in_level, std::vector<double> knock_out_levels,
                       double upper_strike, double lower_strike, std::vector<Date> observation_dates,
                       KnockInObservationMode knock_in_observation_mode, BarrierTouchStatus touch_status,
                       double principal_ratio, Date effective_date, Date expiry_date)
        : note_(initial_spot, std::move(knock_out_levels), upper_strike, lower_strike,
                std::move(observation_dates), principal_ratio, touch_status, effective_date, expiry_date),
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
template <typename Note>
[[nodiscard]] inline Result<void> validate_autocallable_note(const Note& note)
{
    if (!std::isfinite(note.initial_spot()) || note.initial_spot() <= 0.0 ||
        !std::isfinite(note.upper_strike()) || note.upper_strike() <= 0.0 ||
        !std::isfinite(note.lower_strike()) || note.lower_strike() < 0.0 ||
        note.lower_strike() > note.upper_strike() ||
        !std::isfinite(note.principal_ratio()) || note.principal_ratio() < 0.0)
        return std::unexpected(Error{ErrorCategory::invalid_parameter, "autocallable terms are invalid"});
    if (note.observation_dates().empty() ||
        note.knock_out_levels().size() != note.observation_dates().size())
        return std::unexpected(Error{ErrorCategory::invalid_schedule, "autocallable schedule is invalid"});
    auto schedule = validate_date_schedule(note.observation_dates(), note.effective_date(), note.expiry_date());
    if (!schedule)
        return std::unexpected(Error{ErrorCategory::invalid_schedule, "autocallable schedule is invalid"});
    for (std::size_t index = 0; index < note.observation_dates().size(); ++index) {
        if (!std::isfinite(note.knock_out_levels()[index]) || note.knock_out_levels()[index] <= 0.0)
            return std::unexpected(Error{ErrorCategory::invalid_parameter, "knock-out levels are invalid"});
    }
    if (note.touch_status() != BarrierTouchStatus::none &&
        note.touch_status() != BarrierTouchStatus::up &&
        note.touch_status() != BarrierTouchStatus::down)
        return std::unexpected(Error{ErrorCategory::invalid_parameter,
                                     "autocallable touch status is invalid"});
    if constexpr (requires { note.knock_in_level(); }) {
        if (!std::isfinite(note.knock_in_level()) || note.knock_in_level() <= 0.0 ||
            (note.knock_in_observation_mode() != KnockInObservationMode::every_trading_day &&
             note.knock_in_observation_mode() != KnockInObservationMode::at_expiry))
            return std::unexpected(Error{ErrorCategory::invalid_parameter, "knock-in terms are invalid"});
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
        if (!std::isfinite(note.minimum_coupon_rate()))
            return std::unexpected(Error{ErrorCategory::invalid_parameter, "minimum coupon is invalid"});
    }
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
