#pragma once

#include <cmath>
#include <vector>
#include <utility>
#include <kiyosi/core/types.hpp>

namespace kiyosi {
enum class observation_frequency { daily,
                                   at_expiry };
enum class barrier_touch_status { none,
                                  no_touch = none,
                                  up,
                                  up_touch = up,
                                  down,
                                  down_touch = down };
using ObservationFrequency = observation_frequency;
using BarrierTouchStatus = barrier_touch_status;

class AutocallableNote {
public:
    double initial_price() const noexcept { return initial_price_; }
    const std::vector<double>& knock_out_prices() const noexcept { return knock_out_prices_; }
    double upper_strike() const noexcept { return upper_strike_; }
    double lower_strike() const noexcept { return lower_strike_; }
    const std::vector<date>& observation_dates() const noexcept { return observation_dates_; }
    double principal_ratio() const noexcept { return principal_ratio_; }
    date effective() const noexcept { return effective_; }
    date expiry() const noexcept { return expiry_; }
    barrier_touch_status touch_status() const noexcept { return touch_status_; }
    friend bool operator==(const AutocallableNote&, const AutocallableNote&) = default;

protected:
    AutocallableNote(double initial_price, std::vector<double> knock_out_prices, double upper_strike,
                     double lower_strike, std::vector<date> observations, double principal_ratio,
                     barrier_touch_status touch_status, date effective, date expiry)
        : initial_price_(initial_price), knock_out_prices_(std::move(knock_out_prices)), upper_strike_(upper_strike),
          lower_strike_(lower_strike), observation_dates_(std::move(observations)), principal_ratio_(principal_ratio),
          touch_status_(touch_status), effective_(effective), expiry_(expiry) {}

private:
    double initial_price_;
    std::vector<double> knock_out_prices_;
    double upper_strike_;
    double lower_strike_;
    std::vector<date> observation_dates_;
    double principal_ratio_;
    barrier_touch_status touch_status_;
    date effective_;
    date expiry_;
};

class KiAutocallableNote : public AutocallableNote {
public:
    double knock_in_price() const noexcept { return knock_in_price_; }
    observation_frequency knock_in_frequency() const noexcept { return frequency_; }

protected:
    KiAutocallableNote(double initial_price, double knock_in_price, std::vector<double> knock_out_prices,
                       double upper_strike, double lower_strike, std::vector<date> observations,
                       observation_frequency frequency, barrier_touch_status touch_status, double principal_ratio,
                       date effective, date expiry)
        : AutocallableNote(initial_price, std::move(knock_out_prices), upper_strike, lower_strike,
                           std::move(observations), principal_ratio, touch_status, effective, expiry),
          knock_in_price_(knock_in_price), frequency_(frequency) {}

private:
    double knock_in_price_;
    observation_frequency frequency_;
};

class Accumulator {
public:
    double strike() const noexcept { return strike_; }
    double knock_out() const noexcept { return knock_out_; }
    double daily_quantity() const noexcept { return daily_quantity_; }
    double acceleration() const noexcept { return acceleration_; }
    double accumulated_quantity() const noexcept { return accumulated_quantity_; }
    date effective() const noexcept { return effective_; }
    date expiry() const noexcept { return expiry_; }
    Accumulator(double strike, double knock_out, double daily_quantity, double acceleration,
                double accumulated_quantity, date effective, date expiry)
        : strike_(strike), knock_out_(knock_out), daily_quantity_(daily_quantity), acceleration_(acceleration),
          accumulated_quantity_(accumulated_quantity), effective_(effective), expiry_(expiry) {}

private:
    double strike_, knock_out_, daily_quantity_, acceleration_, accumulated_quantity_;
    date effective_, expiry_;
};

class PhoenixOption : public KiAutocallableNote {
public:
    PhoenixOption(double coupon_rate, double initial_price, double knock_in_price, std::vector<double> knock_out_prices,
                  std::vector<double> coupon_barriers, double upper_strike, double lower_strike,
                  std::vector<date> observations, observation_frequency frequency, barrier_touch_status touch_status,
                  double principal_ratio, date effective, date expiry)
        : KiAutocallableNote(initial_price, knock_in_price, std::move(knock_out_prices), upper_strike, lower_strike,
                             observations, frequency, touch_status, principal_ratio, effective, expiry),
          coupon_rate_(coupon_rate), coupon_barriers_(std::move(coupon_barriers)) {}
    double coupon_rate() const noexcept { return coupon_rate_; }
    const std::vector<double>& coupon_barriers() const noexcept { return coupon_barriers_; }

private:
    double coupon_rate_;
    std::vector<double> coupon_barriers_;
};

class SnowballOption : public KiAutocallableNote {
public:
    SnowballOption(std::vector<double> knock_out_coupon_rates, double maturity_coupon_rate,
                   double initial_price, double knock_in_price, std::vector<double> knock_out_prices,
                   double upper_strike, double lower_strike, std::vector<date> observations,
                   observation_frequency frequency, barrier_touch_status touch_status, double principal_ratio,
                   date effective, date expiry)
        : KiAutocallableNote(initial_price, knock_in_price, std::move(knock_out_prices), upper_strike, lower_strike,
                             std::move(observations), frequency, touch_status, principal_ratio, effective, expiry),
          knock_out_coupon_rates_(std::move(knock_out_coupon_rates)), maturity_coupon_rate_(maturity_coupon_rate) {}
    const std::vector<double>& knock_out_coupon_rates() const noexcept { return knock_out_coupon_rates_; }
    double maturity_coupon_rate() const noexcept { return maturity_coupon_rate_; }

private:
    std::vector<double> knock_out_coupon_rates_;
    double maturity_coupon_rate_;
};

class BinarySnowballOption : public AutocallableNote {
public:
    BinarySnowballOption(std::vector<double> knock_out_coupon_rates, double maturity_coupon_rate,
                         double initial_price, std::vector<double> knock_out_prices, double upper_strike,
                         double lower_strike, std::vector<date> observations, barrier_touch_status touch_status,
                         double principal_ratio, date effective, date expiry)
        : AutocallableNote(initial_price, std::move(knock_out_prices), upper_strike, lower_strike,
                           std::move(observations), principal_ratio, touch_status, effective, expiry),
          knock_out_coupon_rates_(std::move(knock_out_coupon_rates)), maturity_coupon_rate_(maturity_coupon_rate) {}
    const std::vector<double>& knock_out_coupon_rates() const noexcept { return knock_out_coupon_rates_; }
    double maturity_coupon_rate() const noexcept { return maturity_coupon_rate_; }

private:
    std::vector<double> knock_out_coupon_rates_;
    double maturity_coupon_rate_;
};

class TernarySnowballOption : public KiAutocallableNote {
public:
    TernarySnowballOption(std::vector<double> knock_out_coupon_rates, double maturity_coupon_rate,
                          double minimal_coupon_rate, double initial_price, double knock_in_price,
                          std::vector<double> knock_out_prices, double upper_strike, double lower_strike,
                          std::vector<date> observations, observation_frequency frequency,
                          barrier_touch_status touch_status, double principal_ratio, date effective, date expiry)
        : KiAutocallableNote(initial_price, knock_in_price, std::move(knock_out_prices), upper_strike, lower_strike,
                             std::move(observations), frequency, touch_status, principal_ratio, effective, expiry),
          knock_out_coupon_rates_(std::move(knock_out_coupon_rates)), maturity_coupon_rate_(maturity_coupon_rate),
          minimal_coupon_rate_(minimal_coupon_rate) {}
    const std::vector<double>& knock_out_coupon_rates() const noexcept { return knock_out_coupon_rates_; }
    double maturity_coupon_rate() const noexcept { return maturity_coupon_rate_; }
    double minimal_coupon_rate() const noexcept { return minimal_coupon_rate_; }

private:
    std::vector<double> knock_out_coupon_rates_;
    double maturity_coupon_rate_, minimal_coupon_rate_;
};

[[nodiscard]] inline result<Accumulator> make_accumulator(
    double strike, double knock_out, double daily_quantity, double acceleration,
    double accumulated_quantity, date effective, date expiry)
{
    if (!std::isfinite(strike) || strike <= 0.0 || !std::isfinite(knock_out) || knock_out <= 0.0 ||
        !std::isfinite(daily_quantity) || daily_quantity < 0.0 || !std::isfinite(acceleration) || acceleration < 0.0 ||
        !std::isfinite(accumulated_quantity) || accumulated_quantity < 0.0)
        return std::unexpected(Error{error_category::invalid_parameter, "accumulator terms are invalid"});
    if (!is_valid_date(effective) || !is_valid_date(expiry) || effective > expiry)
        return std::unexpected(Error{error_category::invalid_schedule, "accumulator dates are invalid"});
    return Accumulator{strike, knock_out, daily_quantity, acceleration, accumulated_quantity, effective, expiry};
}

template <typename Note>
[[nodiscard]] inline result<Note> validate_note(Note note)
{
    if (!std::isfinite(note.initial_price()) || note.initial_price() <= 0.0 ||
        !std::isfinite(note.upper_strike()) || note.upper_strike() <= 0.0 ||
        !std::isfinite(note.lower_strike()) || note.lower_strike() < 0.0 || note.lower_strike() > note.upper_strike() ||
        !std::isfinite(note.principal_ratio()) || note.principal_ratio() < 0.0 ||
        note.observation_dates().empty() || note.knock_out_prices().size() != note.observation_dates().size())
        return std::unexpected(Error{error_category::invalid_parameter, "autocallable terms are invalid"});
    return note;
}
} // namespace kiyosi
