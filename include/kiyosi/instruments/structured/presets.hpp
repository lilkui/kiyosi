#pragma once

#include <utility>
#include <vector>

#include <kiyosi/instruments/structured/snowball.hpp>

namespace kiyosi {

/// Named market conventions layered over make_snowball_option; each one only shapes the
/// coupon and knock-out ladders before delegating to the authoritative factory.

struct StandardSnowballTerms {
    double coupon_rate{};
    double initial_price{};
    double knock_in_price{};
    double knock_out_price{};
    std::vector<date> observation_dates;
    date effective{};
    date expiry{};
    barrier_touch_status touch_status{SnowballTerms{}.touch_status};
    double principal_ratio{SnowballTerms{}.principal_ratio};
};

[[nodiscard]] inline result<SnowballOption> make_standard_snowball(
    StandardSnowballTerms terms)
{
    return make_snowball_option({.knock_out_coupon_rates =
                                     std::vector<double>(terms.observation_dates.size(), terms.coupon_rate),
                                 .maturity_coupon_rate = terms.coupon_rate,
                                 .initial_price = terms.initial_price,
                                 .knock_in_price = terms.knock_in_price,
                                 .knock_out_prices =
                                     std::vector<double>(terms.observation_dates.size(), terms.knock_out_price),
                                 .upper_strike = terms.initial_price,
                                 .lower_strike = 0.0,
                                 .observation_dates = std::move(terms.observation_dates),
                                 .frequency = observation_frequency::daily,
                                 .touch_status = terms.touch_status,
                                 .principal_ratio = terms.principal_ratio,
                                 .effective = terms.effective,
                                 .expiry = terms.expiry});
}

struct StepDownSnowballTerms {
    double coupon_rate{};
    double initial_price{};
    double knock_in_price{};
    double knock_out_start{};
    /// Absolute underlying-price decrement applied at each observation.
    double knock_out_step{};
    std::vector<date> observation_dates;
    date effective{};
    date expiry{};
    barrier_touch_status touch_status{SnowballTerms{}.touch_status};
    double principal_ratio{SnowballTerms{}.principal_ratio};
};

[[nodiscard]] inline result<SnowballOption> make_step_down_snowball(
    StepDownSnowballTerms terms)
{
    std::vector<double> knock_out_prices;
    knock_out_prices.reserve(terms.observation_dates.size());
    for (std::size_t index = 0; index < terms.observation_dates.size(); ++index)
        knock_out_prices.push_back(terms.knock_out_start - static_cast<double>(index) * terms.knock_out_step);
    return make_snowball_option({.knock_out_coupon_rates =
                                     std::vector<double>(terms.observation_dates.size(), terms.coupon_rate),
                                 .maturity_coupon_rate = terms.coupon_rate,
                                 .initial_price = terms.initial_price,
                                 .knock_in_price = terms.knock_in_price,
                                 .knock_out_prices = std::move(knock_out_prices),
                                 .upper_strike = terms.initial_price,
                                 .lower_strike = 0.0,
                                 .observation_dates = std::move(terms.observation_dates),
                                 .frequency = observation_frequency::daily,
                                 .touch_status = terms.touch_status,
                                 .principal_ratio = terms.principal_ratio,
                                 .effective = terms.effective,
                                 .expiry = terms.expiry});
}

struct BothDownSnowballTerms {
    double coupon_start{};
    /// Absolute decimal-rate decrement applied at each observation (0.01 is one percentage point).
    double coupon_step{};
    double initial_price{};
    double knock_in_price{};
    double knock_out_start{};
    /// Absolute underlying-price decrement applied at each observation.
    double knock_out_step{};
    std::vector<date> observation_dates;
    date effective{};
    date expiry{};
    barrier_touch_status touch_status{SnowballTerms{}.touch_status};
    double principal_ratio{SnowballTerms{}.principal_ratio};
};

[[nodiscard]] inline result<SnowballOption> make_both_down_snowball(
    BothDownSnowballTerms terms)
{
    std::vector<double> coupons;
    std::vector<double> knock_out_prices;
    coupons.reserve(terms.observation_dates.size());
    knock_out_prices.reserve(terms.observation_dates.size());
    for (std::size_t index = 0; index < terms.observation_dates.size(); ++index) {
        coupons.push_back(terms.coupon_start - static_cast<double>(index) * terms.coupon_step);
        knock_out_prices.push_back(terms.knock_out_start - static_cast<double>(index) * terms.knock_out_step);
    }
    const double maturity_coupon = coupons.empty() ? 0.0 : coupons.back();
    return make_snowball_option({.knock_out_coupon_rates = std::move(coupons),
                                 .maturity_coupon_rate = maturity_coupon,
                                 .initial_price = terms.initial_price,
                                 .knock_in_price = terms.knock_in_price,
                                 .knock_out_prices = std::move(knock_out_prices),
                                 .upper_strike = terms.initial_price,
                                 .lower_strike = 0.0,
                                 .observation_dates = std::move(terms.observation_dates),
                                 .frequency = observation_frequency::daily,
                                 .touch_status = terms.touch_status,
                                 .principal_ratio = terms.principal_ratio,
                                 .effective = terms.effective,
                                 .expiry = terms.expiry});
}

struct DualCouponSnowballTerms {
    double knock_out_coupon{};
    double maturity_coupon{};
    double initial_price{};
    double knock_in_price{};
    double knock_out_price{};
    std::vector<date> observation_dates;
    date effective{};
    date expiry{};
    barrier_touch_status touch_status{SnowballTerms{}.touch_status};
    double principal_ratio{SnowballTerms{}.principal_ratio};
};

[[nodiscard]] inline result<SnowballOption> make_dual_coupon_snowball(
    DualCouponSnowballTerms terms)
{
    return make_snowball_option({.knock_out_coupon_rates =
                                     std::vector<double>(terms.observation_dates.size(), terms.knock_out_coupon),
                                 .maturity_coupon_rate = terms.maturity_coupon,
                                 .initial_price = terms.initial_price,
                                 .knock_in_price = terms.knock_in_price,
                                 .knock_out_prices =
                                     std::vector<double>(terms.observation_dates.size(), terms.knock_out_price),
                                 .upper_strike = terms.initial_price,
                                 .lower_strike = 0.0,
                                 .observation_dates = std::move(terms.observation_dates),
                                 .frequency = observation_frequency::daily,
                                 .touch_status = terms.touch_status,
                                 .principal_ratio = terms.principal_ratio,
                                 .effective = terms.effective,
                                 .expiry = terms.expiry});
}

struct ParachuteSnowballTerms {
    double coupon_rate{};
    double initial_price{};
    double knock_in_price{};
    double knock_out_price{};
    double final_knock_out_price{};
    std::vector<date> observation_dates;
    date effective{};
    date expiry{};
    barrier_touch_status touch_status{SnowballTerms{}.touch_status};
    double principal_ratio{SnowballTerms{}.principal_ratio};
};

[[nodiscard]] inline result<SnowballOption> make_parachute_snowball(
    ParachuteSnowballTerms terms)
{
    std::vector<double> knock_out_prices(terms.observation_dates.size(), terms.knock_out_price);
    if (!knock_out_prices.empty()) knock_out_prices.back() = terms.final_knock_out_price;
    return make_snowball_option({.knock_out_coupon_rates =
                                     std::vector<double>(terms.observation_dates.size(), terms.coupon_rate),
                                 .maturity_coupon_rate = terms.coupon_rate,
                                 .initial_price = terms.initial_price,
                                 .knock_in_price = terms.knock_in_price,
                                 .knock_out_prices = std::move(knock_out_prices),
                                 .upper_strike = terms.initial_price,
                                 .lower_strike = 0.0,
                                 .observation_dates = std::move(terms.observation_dates),
                                 .frequency = observation_frequency::daily,
                                 .touch_status = terms.touch_status,
                                 .principal_ratio = terms.principal_ratio,
                                 .effective = terms.effective,
                                 .expiry = terms.expiry});
}

struct OtmSnowballTerms {
    double coupon_rate{};
    double initial_price{};
    double knock_in_price{};
    double knock_out_price{};
    double upper_strike{};
    std::vector<date> observation_dates;
    date effective{};
    date expiry{};
    barrier_touch_status touch_status{SnowballTerms{}.touch_status};
    double principal_ratio{SnowballTerms{}.principal_ratio};
};

[[nodiscard]] inline result<SnowballOption> make_otm_snowball(
    OtmSnowballTerms terms)
{
    return make_snowball_option({.knock_out_coupon_rates =
                                     std::vector<double>(terms.observation_dates.size(), terms.coupon_rate),
                                 .maturity_coupon_rate = terms.coupon_rate,
                                 .initial_price = terms.initial_price,
                                 .knock_in_price = terms.knock_in_price,
                                 .knock_out_prices =
                                     std::vector<double>(terms.observation_dates.size(), terms.knock_out_price),
                                 .upper_strike = terms.upper_strike,
                                 .lower_strike = 0.0,
                                 .observation_dates = std::move(terms.observation_dates),
                                 .frequency = observation_frequency::daily,
                                 .touch_status = terms.touch_status,
                                 .principal_ratio = terms.principal_ratio,
                                 .effective = terms.effective,
                                 .expiry = terms.expiry});
}

struct LossCappedSnowballTerms {
    double coupon_rate{};
    double initial_price{};
    double knock_in_price{};
    double knock_out_price{};
    double lower_strike{};
    std::vector<date> observation_dates;
    date effective{};
    date expiry{};
    barrier_touch_status touch_status{SnowballTerms{}.touch_status};
    double principal_ratio{SnowballTerms{}.principal_ratio};
};

[[nodiscard]] inline result<SnowballOption> make_loss_capped_snowball(
    LossCappedSnowballTerms terms)
{
    return make_snowball_option({.knock_out_coupon_rates =
                                     std::vector<double>(terms.observation_dates.size(), terms.coupon_rate),
                                 .maturity_coupon_rate = terms.coupon_rate,
                                 .initial_price = terms.initial_price,
                                 .knock_in_price = terms.knock_in_price,
                                 .knock_out_prices =
                                     std::vector<double>(terms.observation_dates.size(), terms.knock_out_price),
                                 .upper_strike = terms.initial_price,
                                 .lower_strike = terms.lower_strike,
                                 .observation_dates = std::move(terms.observation_dates),
                                 .frequency = observation_frequency::daily,
                                 .touch_status = terms.touch_status,
                                 .principal_ratio = terms.principal_ratio,
                                 .effective = terms.effective,
                                 .expiry = terms.expiry});
}

struct EuropeanSnowballTerms {
    double coupon_rate{};
    double initial_price{};
    double knock_in_price{};
    double knock_out_price{};
    std::vector<date> observation_dates;
    date effective{};
    date expiry{};
    barrier_touch_status touch_status{SnowballTerms{}.touch_status};
    double principal_ratio{SnowballTerms{}.principal_ratio};
};

[[nodiscard]] inline result<SnowballOption> make_european_snowball(
    EuropeanSnowballTerms terms)
{
    return make_snowball_option({.knock_out_coupon_rates =
                                     std::vector<double>(terms.observation_dates.size(), terms.coupon_rate),
                                 .maturity_coupon_rate = terms.coupon_rate,
                                 .initial_price = terms.initial_price,
                                 .knock_in_price = terms.knock_in_price,
                                 .knock_out_prices =
                                     std::vector<double>(terms.observation_dates.size(), terms.knock_out_price),
                                 .upper_strike = terms.initial_price,
                                 .lower_strike = 0.0,
                                 .observation_dates = std::move(terms.observation_dates),
                                 .frequency = observation_frequency::at_expiry,
                                 .touch_status = terms.touch_status,
                                 .principal_ratio = terms.principal_ratio,
                                 .effective = terms.effective,
                                 .expiry = terms.expiry});
}

} // namespace kiyosi
