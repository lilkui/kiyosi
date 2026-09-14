#pragma once

#include <utility>
#include <vector>

#include <kiyosi/instruments/structured/snowball.hpp>

namespace kiyosi {

/// Named market conventions layered over make_snowball_option; each one only shapes the
/// coupon and knock-out ladders before delegating to the authoritative factory.

[[nodiscard]] inline result<SnowballOption> make_standard_snowball(
    double coupon_rate, double initial_price, double knock_in_price, double knock_out_price,
    std::vector<date> observations, date effective, date expiry,
    barrier_touch_status touch_status = barrier_touch_status::none, double principal_ratio = 1.0)
{
    return make_snowball_option({
        std::vector<double>(observations.size(), coupon_rate), coupon_rate, initial_price,
        knock_in_price, std::vector<double>(observations.size(), knock_out_price), initial_price, 0.0,
        observations, observation_frequency::daily, touch_status, principal_ratio, effective, expiry});
}

[[nodiscard]] inline result<SnowballOption> make_step_down_snowball(
    double coupon_rate, double initial_price, double knock_in_price, double knock_out_start,
    double knock_out_step, std::vector<date> observations, date effective, date expiry,
    barrier_touch_status touch_status = barrier_touch_status::none, double principal_ratio = 1.0)
{
    std::vector<double> knock_out_prices;
    knock_out_prices.reserve(observations.size());
    for (std::size_t index = 0; index < observations.size(); ++index)
        knock_out_prices.push_back(knock_out_start - static_cast<double>(index) * knock_out_step);
    return make_snowball_option({
        std::vector<double>(observations.size(), coupon_rate), coupon_rate, initial_price,
        knock_in_price, std::move(knock_out_prices), initial_price, 0.0, observations,
        observation_frequency::daily, touch_status, principal_ratio, effective, expiry});
}

[[nodiscard]] inline result<SnowballOption> make_both_down_snowball(
    double coupon_start, double coupon_step, double initial_price, double knock_in_price,
    double knock_out_start, double knock_out_step, std::vector<date> observations, date effective,
    date expiry, barrier_touch_status touch_status = barrier_touch_status::none,
    double principal_ratio = 1.0)
{
    std::vector<double> coupons;
    std::vector<double> knock_out_prices;
    coupons.reserve(observations.size());
    knock_out_prices.reserve(observations.size());
    for (std::size_t index = 0; index < observations.size(); ++index) {
        coupons.push_back(coupon_start - static_cast<double>(index) * coupon_step);
        knock_out_prices.push_back(knock_out_start - static_cast<double>(index) * knock_out_step);
    }
    const double maturity_coupon = coupons.empty() ? 0.0 : coupons.back();
    return make_snowball_option({
        std::move(coupons), maturity_coupon, initial_price, knock_in_price,
        std::move(knock_out_prices), initial_price, 0.0, observations, observation_frequency::daily,
        touch_status, principal_ratio, effective, expiry});
}

[[nodiscard]] inline result<SnowballOption> make_dual_coupon_snowball(
    double knock_out_coupon, double maturity_coupon, double initial_price, double knock_in_price,
    double knock_out_price, std::vector<date> observations, date effective, date expiry,
    barrier_touch_status touch_status = barrier_touch_status::none, double principal_ratio = 1.0)
{
    return make_snowball_option({
        std::vector<double>(observations.size(), knock_out_coupon), maturity_coupon, initial_price,
        knock_in_price, std::vector<double>(observations.size(), knock_out_price), initial_price, 0.0,
        observations, observation_frequency::daily, touch_status, principal_ratio, effective, expiry});
}

[[nodiscard]] inline result<SnowballOption> make_parachute_snowball(
    double coupon_rate, double initial_price, double knock_in_price, double knock_out_price,
    double final_knock_out_price, std::vector<date> observations, date effective, date expiry,
    barrier_touch_status touch_status = barrier_touch_status::none, double principal_ratio = 1.0)
{
    std::vector<double> knock_out_prices(observations.size(), knock_out_price);
    if (!knock_out_prices.empty()) knock_out_prices.back() = final_knock_out_price;
    return make_snowball_option({
        std::vector<double>(observations.size(), coupon_rate), coupon_rate, initial_price,
        knock_in_price, std::move(knock_out_prices), initial_price, 0.0, observations,
        observation_frequency::daily, touch_status, principal_ratio, effective, expiry});
}

[[nodiscard]] inline result<SnowballOption> make_otm_snowball(
    double coupon_rate, double initial_price, double knock_in_price, double knock_out_price,
    double upper_strike, std::vector<date> observations, date effective, date expiry,
    barrier_touch_status touch_status = barrier_touch_status::none, double principal_ratio = 1.0)
{
    return make_snowball_option({
        std::vector<double>(observations.size(), coupon_rate), coupon_rate, initial_price,
        knock_in_price, std::vector<double>(observations.size(), knock_out_price), upper_strike, 0.0,
        observations, observation_frequency::daily, touch_status, principal_ratio, effective, expiry});
}

[[nodiscard]] inline result<SnowballOption> make_loss_capped_snowball(
    double coupon_rate, double initial_price, double knock_in_price, double knock_out_price,
    double lower_strike, std::vector<date> observations, date effective, date expiry,
    barrier_touch_status touch_status = barrier_touch_status::none, double principal_ratio = 1.0)
{
    return make_snowball_option({
        std::vector<double>(observations.size(), coupon_rate), coupon_rate, initial_price,
        knock_in_price, std::vector<double>(observations.size(), knock_out_price), initial_price,
        lower_strike, observations, observation_frequency::daily, touch_status, principal_ratio,
        effective, expiry});
}

[[nodiscard]] inline result<SnowballOption> make_european_snowball(
    double coupon_rate, double initial_price, double knock_in_price, double knock_out_price,
    std::vector<date> observations, date effective, date expiry,
    barrier_touch_status touch_status = barrier_touch_status::none, double principal_ratio = 1.0)
{
    return make_snowball_option({
        std::vector<double>(observations.size(), coupon_rate), coupon_rate, initial_price,
        knock_in_price, std::vector<double>(observations.size(), knock_out_price), initial_price, 0.0,
        observations, observation_frequency::at_expiry, touch_status, principal_ratio, effective,
        expiry});
}

} // namespace kiyosi
