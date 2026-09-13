#pragma once

#include <utility>
#include <vector>

#include <kiyosi/instruments/structured/autocallable.hpp>

namespace kiyosi {

class PhoenixOption;

[[nodiscard]] result<PhoenixOption> make_phoenix_option(
    double, double, double, std::vector<double>, std::vector<double>, double, double,
    std::vector<date>, observation_frequency, barrier_touch_status, double, date, date);

/// Knock-in autocallable paying a conditional coupon whenever spot clears the coupon barrier.
class PhoenixOption {
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
    double knock_in_price() const noexcept { return note_.knock_in_price(); }
    observation_frequency knock_in_frequency() const noexcept { return note_.knock_in_frequency(); }
    double coupon_rate() const noexcept { return coupon_rate_; }
    const std::vector<double>& coupon_barriers() const noexcept { return coupon_barriers_; }

    result<PhoenixOption> with_coupon_rate(double coupon) const;

private:
    PhoenixOption(double coupon_rate, double initial_price, double knock_in_price,
                  std::vector<double> knock_out_prices, std::vector<double> coupon_barriers,
                  double upper_strike, double lower_strike, std::vector<date> observations,
                  observation_frequency frequency, barrier_touch_status touch_status,
                  double principal_ratio, date effective, date expiry)
        : note_(initial_price, knock_in_price, std::move(knock_out_prices), upper_strike, lower_strike,
                std::move(observations), frequency, touch_status, principal_ratio, effective, expiry),
          coupon_rate_(coupon_rate), coupon_barriers_(std::move(coupon_barriers)) {}

    KiAutocallableNote note_;
    double coupon_rate_;
    std::vector<double> coupon_barriers_;

    friend result<PhoenixOption> make_phoenix_option(
        double, double, double, std::vector<double>, std::vector<double>, double, double,
        std::vector<date>, observation_frequency, barrier_touch_status, double, date, date);
};

[[nodiscard]] inline result<PhoenixOption> make_phoenix_option(
    double coupon_rate, double initial_price, double knock_in_price,
    std::vector<double> knock_out_prices, std::vector<double> coupon_barriers, double upper_strike,
    double lower_strike, std::vector<date> observations, observation_frequency frequency,
    barrier_touch_status touch_status, double principal_ratio, date effective, date expiry)
{
    return validate_note(PhoenixOption{coupon_rate, initial_price, knock_in_price,
                                       std::move(knock_out_prices), std::move(coupon_barriers),
                                       upper_strike, lower_strike, std::move(observations), frequency,
                                       touch_status, principal_ratio, effective, expiry});
}

inline result<PhoenixOption> PhoenixOption::with_coupon_rate(double coupon) const
{
    return make_phoenix_option(coupon, initial_price(), knock_in_price(), knock_out_prices(),
                               coupon_barriers(), upper_strike(), lower_strike(), observation_dates(),
                               knock_in_frequency(), touch_status(), principal_ratio(), effective(),
                               expiry());
}

} // namespace kiyosi
