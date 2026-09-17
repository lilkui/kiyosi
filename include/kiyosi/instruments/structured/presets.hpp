#pragma once

#include <utility>
#include <vector>

#include <kiyosi/instruments/structured/snowball.hpp>

namespace kiyosi {

/// Named market conventions layered over make_snowball_option; each one only shapes the
/// coupon and knock-out ladders before delegating to the authoritative factory.

[[nodiscard]] inline result<SnowballOption> make_standard_snowball(
    double coupon_rate, double initial_price, double knock_in_price, double knock_out_price,
    std::vector<date> observation_dates, date effective, date expiry,
    barrier_touch_status touch_status = SnowballTerms{}.touch_status,
    double principal_ratio = SnowballTerms{}.principal_ratio)
{
    return make_snowball_option({.knock_out_coupon_rates = std::vector<double>(observation_dates.size(), coupon_rate),
                                 .maturity_coupon_rate = coupon_rate,
                                 .initial_price = initial_price,
                                 .knock_in_price = knock_in_price,
                                 .knock_out_prices = std::vector<double>(observation_dates.size(), knock_out_price),
                                 .upper_strike = initial_price,
                                 .lower_strike = 0.0,
                                 .observation_dates = observation_dates,
                                 .frequency = observation_frequency::daily,
                                 .touch_status = touch_status,
                                 .principal_ratio = principal_ratio,
                                 .effective = effective,
                                 .expiry = expiry});
}

[[nodiscard]] inline result<SnowballOption> make_step_down_snowball(
    double coupon_rate, double initial_price, double knock_in_price, double knock_out_start,
    double knock_out_step, std::vector<date> observation_dates, date effective, date expiry,
    barrier_touch_status touch_status = SnowballTerms{}.touch_status,
    double principal_ratio = SnowballTerms{}.principal_ratio)
{
    std::vector<double> knock_out_prices;
    knock_out_prices.reserve(observation_dates.size());
    for (std::size_t index = 0; index < observation_dates.size(); ++index)
        knock_out_prices.push_back(knock_out_start - static_cast<double>(index) * knock_out_step);
    return make_snowball_option({.knock_out_coupon_rates = std::vector<double>(observation_dates.size(), coupon_rate),
                                 .maturity_coupon_rate = coupon_rate,
                                 .initial_price = initial_price,
                                 .knock_in_price = knock_in_price,
                                 .knock_out_prices = std::move(knock_out_prices),
                                 .upper_strike = initial_price,
                                 .lower_strike = 0.0,
                                 .observation_dates = observation_dates,
                                 .frequency = observation_frequency::daily,
                                 .touch_status = touch_status,
                                 .principal_ratio = principal_ratio,
                                 .effective = effective,
                                 .expiry = expiry});
}

[[nodiscard]] inline result<SnowballOption> make_both_down_snowball(
    double coupon_start, double coupon_step, double initial_price, double knock_in_price,
    double knock_out_start, double knock_out_step, std::vector<date> observation_dates, date effective,
    date expiry, barrier_touch_status touch_status = SnowballTerms{}.touch_status,
    double principal_ratio = SnowballTerms{}.principal_ratio)
{
    std::vector<double> coupons;
    std::vector<double> knock_out_prices;
    coupons.reserve(observation_dates.size());
    knock_out_prices.reserve(observation_dates.size());
    for (std::size_t index = 0; index < observation_dates.size(); ++index) {
        coupons.push_back(coupon_start - static_cast<double>(index) * coupon_step);
        knock_out_prices.push_back(knock_out_start - static_cast<double>(index) * knock_out_step);
    }
    const double maturity_coupon = coupons.empty() ? 0.0 : coupons.back();
    return make_snowball_option({.knock_out_coupon_rates = std::move(coupons),
                                 .maturity_coupon_rate = maturity_coupon,
                                 .initial_price = initial_price,
                                 .knock_in_price = knock_in_price,
                                 .knock_out_prices = std::move(knock_out_prices),
                                 .upper_strike = initial_price,
                                 .lower_strike = 0.0,
                                 .observation_dates = observation_dates,
                                 .frequency = observation_frequency::daily,
                                 .touch_status = touch_status,
                                 .principal_ratio = principal_ratio,
                                 .effective = effective,
                                 .expiry = expiry});
}

[[nodiscard]] inline result<SnowballOption> make_dual_coupon_snowball(
    double knock_out_coupon, double maturity_coupon, double initial_price, double knock_in_price,
    double knock_out_price, std::vector<date> observation_dates, date effective, date expiry,
    barrier_touch_status touch_status = SnowballTerms{}.touch_status,
    double principal_ratio = SnowballTerms{}.principal_ratio)
{
    return make_snowball_option({.knock_out_coupon_rates = std::vector<double>(observation_dates.size(), knock_out_coupon),
                                 .maturity_coupon_rate = maturity_coupon,
                                 .initial_price = initial_price,
                                 .knock_in_price = knock_in_price,
                                 .knock_out_prices = std::vector<double>(observation_dates.size(), knock_out_price),
                                 .upper_strike = initial_price,
                                 .lower_strike = 0.0,
                                 .observation_dates = observation_dates,
                                 .frequency = observation_frequency::daily,
                                 .touch_status = touch_status,
                                 .principal_ratio = principal_ratio,
                                 .effective = effective,
                                 .expiry = expiry});
}

[[nodiscard]] inline result<SnowballOption> make_parachute_snowball(
    double coupon_rate, double initial_price, double knock_in_price, double knock_out_price,
    double final_knock_out_price, std::vector<date> observation_dates, date effective, date expiry,
    barrier_touch_status touch_status = SnowballTerms{}.touch_status,
    double principal_ratio = SnowballTerms{}.principal_ratio)
{
    std::vector<double> knock_out_prices(observation_dates.size(), knock_out_price);
    if (!knock_out_prices.empty()) knock_out_prices.back() = final_knock_out_price;
    return make_snowball_option({.knock_out_coupon_rates = std::vector<double>(observation_dates.size(), coupon_rate),
                                 .maturity_coupon_rate = coupon_rate,
                                 .initial_price = initial_price,
                                 .knock_in_price = knock_in_price,
                                 .knock_out_prices = std::move(knock_out_prices),
                                 .upper_strike = initial_price,
                                 .lower_strike = 0.0,
                                 .observation_dates = observation_dates,
                                 .frequency = observation_frequency::daily,
                                 .touch_status = touch_status,
                                 .principal_ratio = principal_ratio,
                                 .effective = effective,
                                 .expiry = expiry});
}

[[nodiscard]] inline result<SnowballOption> make_otm_snowball(
    double coupon_rate, double initial_price, double knock_in_price, double knock_out_price,
    double upper_strike, std::vector<date> observation_dates, date effective, date expiry,
    barrier_touch_status touch_status = SnowballTerms{}.touch_status,
    double principal_ratio = SnowballTerms{}.principal_ratio)
{
    return make_snowball_option({.knock_out_coupon_rates = std::vector<double>(observation_dates.size(), coupon_rate),
                                 .maturity_coupon_rate = coupon_rate,
                                 .initial_price = initial_price,
                                 .knock_in_price = knock_in_price,
                                 .knock_out_prices = std::vector<double>(observation_dates.size(), knock_out_price),
                                 .upper_strike = upper_strike,
                                 .lower_strike = 0.0,
                                 .observation_dates = observation_dates,
                                 .frequency = observation_frequency::daily,
                                 .touch_status = touch_status,
                                 .principal_ratio = principal_ratio,
                                 .effective = effective,
                                 .expiry = expiry});
}

[[nodiscard]] inline result<SnowballOption> make_loss_capped_snowball(
    double coupon_rate, double initial_price, double knock_in_price, double knock_out_price,
    double lower_strike, std::vector<date> observation_dates, date effective, date expiry,
    barrier_touch_status touch_status = SnowballTerms{}.touch_status,
    double principal_ratio = SnowballTerms{}.principal_ratio)
{
    return make_snowball_option({.knock_out_coupon_rates = std::vector<double>(observation_dates.size(), coupon_rate),
                                 .maturity_coupon_rate = coupon_rate,
                                 .initial_price = initial_price,
                                 .knock_in_price = knock_in_price,
                                 .knock_out_prices = std::vector<double>(observation_dates.size(), knock_out_price),
                                 .upper_strike = initial_price,
                                 .lower_strike = lower_strike,
                                 .observation_dates = observation_dates,
                                 .frequency = observation_frequency::daily,
                                 .touch_status = touch_status,
                                 .principal_ratio = principal_ratio,
                                 .effective = effective,
                                 .expiry = expiry});
}

[[nodiscard]] inline result<SnowballOption> make_european_snowball(
    double coupon_rate, double initial_price, double knock_in_price, double knock_out_price,
    std::vector<date> observation_dates, date effective, date expiry,
    barrier_touch_status touch_status = SnowballTerms{}.touch_status,
    double principal_ratio = SnowballTerms{}.principal_ratio)
{
    return make_snowball_option({.knock_out_coupon_rates = std::vector<double>(observation_dates.size(), coupon_rate),
                                 .maturity_coupon_rate = coupon_rate,
                                 .initial_price = initial_price,
                                 .knock_in_price = knock_in_price,
                                 .knock_out_prices = std::vector<double>(observation_dates.size(), knock_out_price),
                                 .upper_strike = initial_price,
                                 .lower_strike = 0.0,
                                 .observation_dates = observation_dates,
                                 .frequency = observation_frequency::at_expiry,
                                 .touch_status = touch_status,
                                 .principal_ratio = principal_ratio,
                                 .effective = effective,
                                 .expiry = expiry});
}

} // namespace kiyosi
