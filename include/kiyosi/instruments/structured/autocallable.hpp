#pragma once

#include <cmath>
#include <utility>
#include <vector>

#include <kiyosi/core/error.hpp>
#include <kiyosi/core/time.hpp>
#include <kiyosi/market/schedule.hpp>

namespace kiyosi {

enum class observation_frequency { daily,
                                   at_expiry };

enum class barrier_touch_status { none,
                                  up,
                                  down };

/// Principal, knock-out ladder, and settlement strikes shared by every autocallable structure.
class AutocallableNote {
public:
    double initial_price() const noexcept { return terms_.initial_price; }
    const std::vector<double>& knock_out_prices() const noexcept { return terms_.knock_out_prices; }
    double upper_strike() const noexcept { return terms_.upper_strike; }
    double lower_strike() const noexcept { return terms_.lower_strike; }
    const std::vector<date>& observation_dates() const noexcept { return terms_.observation_dates; }
    double principal_ratio() const noexcept { return terms_.principal_ratio; }
    date effective() const noexcept { return terms_.effective; }
    date expiry() const noexcept { return terms_.expiry; }
    barrier_touch_status touch_status() const noexcept { return terms_.touch_status; }
    friend bool operator==(const AutocallableNote&, const AutocallableNote&) = default;

private:
    AutocallableNote(double initial_price, std::vector<double> knock_out_prices, double upper_strike,
                     double lower_strike, std::vector<date> observations, double principal_ratio,
                     barrier_touch_status touch_status, date effective, date expiry)
        : terms_{initial_price, std::move(knock_out_prices), upper_strike, lower_strike,
                 std::move(observations), principal_ratio, touch_status, effective, expiry} {}

    struct Terms {
        double initial_price;
        std::vector<double> knock_out_prices;
        double upper_strike;
        double lower_strike;
        std::vector<date> observation_dates;
        double principal_ratio;
        barrier_touch_status touch_status;
        date effective;
        date expiry;
        friend bool operator==(const Terms&, const Terms&) = default;
    } terms_;

    friend class KiAutocallableNote;
    friend class BinarySnowballOption;
};

/// An autocallable note carrying a downside knock-in barrier.
class KiAutocallableNote {
public:
    double initial_price() const noexcept { return note_.initial_price(); }
    const std::vector<double>& knock_out_prices() const noexcept { return note_.knock_out_prices(); }
    double upper_strike() const noexcept { return note_.upper_strike(); }
    double lower_strike() const noexcept { return note_.lower_strike(); }
    const std::vector<date>& observation_dates() const noexcept { return note_.observation_dates(); }
    double principal_ratio() const noexcept { return note_.principal_ratio(); }
    date effective() const noexcept { return note_.effective(); }
    date expiry() const noexcept { return note_.expiry(); }
    barrier_touch_status touch_status() const noexcept { return note_.touch_status(); }
    double knock_in_price() const noexcept { return knock_in_price_; }
    observation_frequency knock_in_frequency() const noexcept { return frequency_; }
    friend bool operator==(const KiAutocallableNote&, const KiAutocallableNote&) = default;

private:
    KiAutocallableNote(double initial_price, double knock_in_price, std::vector<double> knock_out_prices,
                       double upper_strike, double lower_strike, std::vector<date> observations,
                       observation_frequency frequency, barrier_touch_status touch_status,
                       double principal_ratio, date effective, date expiry)
        : note_(initial_price, std::move(knock_out_prices), upper_strike, lower_strike,
                std::move(observations), principal_ratio, touch_status, effective, expiry),
          knock_in_price_(knock_in_price), frequency_(frequency) {}

    AutocallableNote note_;
    double knock_in_price_;
    observation_frequency frequency_;

    friend class PhoenixOption;
    friend class SnowballOption;
    friend class TernarySnowballOption;
};

/// Authoritative domain validation for every autocallable product; optional features are
/// detected structurally so each product only pays for the checks it needs.
template <typename Note>
[[nodiscard]] inline result<Note> validate_note(Note note)
{
    if (!std::isfinite(note.initial_price()) || note.initial_price() <= 0.0 ||
        !std::isfinite(note.upper_strike()) || note.upper_strike() <= 0.0 ||
        !std::isfinite(note.lower_strike()) || note.lower_strike() < 0.0 ||
        note.lower_strike() > note.upper_strike() ||
        !std::isfinite(note.principal_ratio()) || note.principal_ratio() < 0.0)
        return std::unexpected(Error{error_category::invalid_parameter, "autocallable terms are invalid"});
    if (note.observation_dates().empty() ||
        note.knock_out_prices().size() != note.observation_dates().size())
        return std::unexpected(Error{error_category::invalid_schedule, "autocallable schedule is invalid"});
    auto schedule = validate_date_schedule(note.observation_dates(), note.effective(), note.expiry());
    if (!schedule)
        return std::unexpected(Error{error_category::invalid_schedule, "autocallable schedule is invalid"});
    for (std::size_t index = 0; index < note.observation_dates().size(); ++index) {
        if (!std::isfinite(note.knock_out_prices()[index]) || note.knock_out_prices()[index] <= 0.0)
            return std::unexpected(Error{error_category::invalid_parameter, "knock-out prices are invalid"});
    }
    if (note.touch_status() != barrier_touch_status::none &&
        note.touch_status() != barrier_touch_status::up &&
        note.touch_status() != barrier_touch_status::down)
        return std::unexpected(Error{error_category::invalid_parameter,
                                     "autocallable touch status is invalid"});
    if constexpr (requires { note.knock_in_price(); }) {
        if (!std::isfinite(note.knock_in_price()) || note.knock_in_price() <= 0.0 ||
            (note.knock_in_frequency() != observation_frequency::daily &&
             note.knock_in_frequency() != observation_frequency::at_expiry))
            return std::unexpected(Error{error_category::invalid_parameter, "knock-in terms are invalid"});
    }
    if constexpr (requires { note.coupon_rate(); }) {
        if (!std::isfinite(note.coupon_rate()))
            return std::unexpected(Error{error_category::invalid_parameter, "Phoenix coupon is invalid"});
        if (note.coupon_barriers().size() != note.observation_dates().size())
            return std::unexpected(Error{error_category::invalid_schedule,
                                         "Phoenix coupon schedule is invalid"});
        for (double barrier : note.coupon_barriers())
            if (!std::isfinite(barrier) || barrier < 0.0)
                return std::unexpected(Error{error_category::invalid_parameter,
                                             "coupon barriers are invalid"});
    }
    if constexpr (requires { note.knock_out_coupon_rates(); }) {
        if (note.knock_out_coupon_rates().size() != note.observation_dates().size())
            return std::unexpected(Error{error_category::invalid_schedule,
                                         "coupon schedule count is invalid"});
        for (double coupon : note.knock_out_coupon_rates())
            if (!std::isfinite(coupon))
                return std::unexpected(Error{error_category::invalid_parameter, "coupon rates are invalid"});
        if (!std::isfinite(note.maturity_coupon_rate()))
            return std::unexpected(Error{error_category::invalid_parameter, "maturity coupon is invalid"});
    }
    if constexpr (requires { note.minimal_coupon_rate(); }) {
        if (!std::isfinite(note.minimal_coupon_rate()))
            return std::unexpected(Error{error_category::invalid_parameter, "minimal coupon is invalid"});
    }
    return note;
}

} // namespace kiyosi
