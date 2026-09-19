#pragma once

#include <utility>
#include <vector>

#include <kiyosi/instruments/structured/snowball.hpp>

namespace kiyosi {

/// Named market conventions layered over make_snowball_option; each one only shapes the
/// coupon and knock-out ladders before delegating to the authoritative factory.

struct StandardSnowballTerms {
    double coupon_rate{};
    double initial_spot{};
    double knock_in_level{};
    double knock_out_level{};
    std::vector<Date> observation_dates;
    Date effective_date{};
    Date expiry_date{};
    BarrierTouchStatus touch_status{SnowballTerms{}.touch_status};
    double principal_ratio{SnowballTerms{}.principal_ratio};
};

[[nodiscard]] inline Result<SnowballOption> make_standard_snowball(
    StandardSnowballTerms terms)
{
    return make_snowball_option({.knock_out_coupon_rates =
                                     std::vector<double>(terms.observation_dates.size(), terms.coupon_rate),
                                 .maturity_coupon_rate = terms.coupon_rate,
                                 .initial_spot = terms.initial_spot,
                                 .knock_in_level = terms.knock_in_level,
                                 .knock_out_levels =
                                     std::vector<double>(terms.observation_dates.size(), terms.knock_out_level),
                                 .upper_strike = terms.initial_spot,
                                 .lower_strike = 0.0,
                                 .observation_dates = std::move(terms.observation_dates),
                                 .knock_in_observation_mode = KnockInObservationMode::every_trading_day,
                                 .touch_status = terms.touch_status,
                                 .principal_ratio = terms.principal_ratio,
                                 .effective_date = terms.effective_date,
                                 .expiry_date = terms.expiry_date});
}

struct StepDownSnowballTerms {
    double coupon_rate{};
    double initial_spot{};
    double knock_in_level{};
    double initial_knock_out_level{};
    /// Absolute underlying-price decrement applied at each observation.
    double knock_out_level_decrement{};
    std::vector<Date> observation_dates;
    Date effective_date{};
    Date expiry_date{};
    BarrierTouchStatus touch_status{SnowballTerms{}.touch_status};
    double principal_ratio{SnowballTerms{}.principal_ratio};
};

[[nodiscard]] inline Result<SnowballOption> make_step_down_snowball(
    StepDownSnowballTerms terms)
{
    std::vector<double> knock_out_levels;
    knock_out_levels.reserve(terms.observation_dates.size());
    for (std::size_t index = 0; index < terms.observation_dates.size(); ++index)
        knock_out_levels.push_back(terms.initial_knock_out_level - static_cast<double>(index) * terms.knock_out_level_decrement);
    return make_snowball_option({.knock_out_coupon_rates =
                                     std::vector<double>(terms.observation_dates.size(), terms.coupon_rate),
                                 .maturity_coupon_rate = terms.coupon_rate,
                                 .initial_spot = terms.initial_spot,
                                 .knock_in_level = terms.knock_in_level,
                                 .knock_out_levels = std::move(knock_out_levels),
                                 .upper_strike = terms.initial_spot,
                                 .lower_strike = 0.0,
                                 .observation_dates = std::move(terms.observation_dates),
                                 .knock_in_observation_mode = KnockInObservationMode::every_trading_day,
                                 .touch_status = terms.touch_status,
                                 .principal_ratio = terms.principal_ratio,
                                 .effective_date = terms.effective_date,
                                 .expiry_date = terms.expiry_date});
}

struct BothDownSnowballTerms {
    double coupon_start{};
    /// Absolute decimal-rate decrement applied at each observation (0.01 is one percentage point).
    double coupon_step{};
    double initial_spot{};
    double knock_in_level{};
    double initial_knock_out_level{};
    /// Absolute underlying-price decrement applied at each observation.
    double knock_out_level_decrement{};
    std::vector<Date> observation_dates;
    Date effective_date{};
    Date expiry_date{};
    BarrierTouchStatus touch_status{SnowballTerms{}.touch_status};
    double principal_ratio{SnowballTerms{}.principal_ratio};
};

[[nodiscard]] inline Result<SnowballOption> make_both_down_snowball(
    BothDownSnowballTerms terms)
{
    std::vector<double> coupons;
    std::vector<double> knock_out_levels;
    coupons.reserve(terms.observation_dates.size());
    knock_out_levels.reserve(terms.observation_dates.size());
    for (std::size_t index = 0; index < terms.observation_dates.size(); ++index) {
        coupons.push_back(terms.coupon_start - static_cast<double>(index) * terms.coupon_step);
        knock_out_levels.push_back(terms.initial_knock_out_level - static_cast<double>(index) * terms.knock_out_level_decrement);
    }
    const double maturity_coupon = coupons.empty() ? 0.0 : coupons.back();
    return make_snowball_option({.knock_out_coupon_rates = std::move(coupons),
                                 .maturity_coupon_rate = maturity_coupon,
                                 .initial_spot = terms.initial_spot,
                                 .knock_in_level = terms.knock_in_level,
                                 .knock_out_levels = std::move(knock_out_levels),
                                 .upper_strike = terms.initial_spot,
                                 .lower_strike = 0.0,
                                 .observation_dates = std::move(terms.observation_dates),
                                 .knock_in_observation_mode = KnockInObservationMode::every_trading_day,
                                 .touch_status = terms.touch_status,
                                 .principal_ratio = terms.principal_ratio,
                                 .effective_date = terms.effective_date,
                                 .expiry_date = terms.expiry_date});
}

struct DualCouponSnowballTerms {
    double knock_out_coupon{};
    double maturity_coupon{};
    double initial_spot{};
    double knock_in_level{};
    double knock_out_level{};
    std::vector<Date> observation_dates;
    Date effective_date{};
    Date expiry_date{};
    BarrierTouchStatus touch_status{SnowballTerms{}.touch_status};
    double principal_ratio{SnowballTerms{}.principal_ratio};
};

[[nodiscard]] inline Result<SnowballOption> make_dual_coupon_snowball(
    DualCouponSnowballTerms terms)
{
    return make_snowball_option({.knock_out_coupon_rates =
                                     std::vector<double>(terms.observation_dates.size(), terms.knock_out_coupon),
                                 .maturity_coupon_rate = terms.maturity_coupon,
                                 .initial_spot = terms.initial_spot,
                                 .knock_in_level = terms.knock_in_level,
                                 .knock_out_levels =
                                     std::vector<double>(terms.observation_dates.size(), terms.knock_out_level),
                                 .upper_strike = terms.initial_spot,
                                 .lower_strike = 0.0,
                                 .observation_dates = std::move(terms.observation_dates),
                                 .knock_in_observation_mode = KnockInObservationMode::every_trading_day,
                                 .touch_status = terms.touch_status,
                                 .principal_ratio = terms.principal_ratio,
                                 .effective_date = terms.effective_date,
                                 .expiry_date = terms.expiry_date});
}

struct ParachuteSnowballTerms {
    double coupon_rate{};
    double initial_spot{};
    double knock_in_level{};
    double knock_out_level{};
    double final_knock_out_level{};
    std::vector<Date> observation_dates;
    Date effective_date{};
    Date expiry_date{};
    BarrierTouchStatus touch_status{SnowballTerms{}.touch_status};
    double principal_ratio{SnowballTerms{}.principal_ratio};
};

[[nodiscard]] inline Result<SnowballOption> make_parachute_snowball(
    ParachuteSnowballTerms terms)
{
    std::vector<double> knock_out_levels(terms.observation_dates.size(), terms.knock_out_level);
    if (!knock_out_levels.empty()) knock_out_levels.back() = terms.final_knock_out_level;
    return make_snowball_option({.knock_out_coupon_rates =
                                     std::vector<double>(terms.observation_dates.size(), terms.coupon_rate),
                                 .maturity_coupon_rate = terms.coupon_rate,
                                 .initial_spot = terms.initial_spot,
                                 .knock_in_level = terms.knock_in_level,
                                 .knock_out_levels = std::move(knock_out_levels),
                                 .upper_strike = terms.initial_spot,
                                 .lower_strike = 0.0,
                                 .observation_dates = std::move(terms.observation_dates),
                                 .knock_in_observation_mode = KnockInObservationMode::every_trading_day,
                                 .touch_status = terms.touch_status,
                                 .principal_ratio = terms.principal_ratio,
                                 .effective_date = terms.effective_date,
                                 .expiry_date = terms.expiry_date});
}

struct OutOfTheMoneySnowballTerms {
    double coupon_rate{};
    double initial_spot{};
    double knock_in_level{};
    double knock_out_level{};
    double upper_strike{};
    std::vector<Date> observation_dates;
    Date effective_date{};
    Date expiry_date{};
    BarrierTouchStatus touch_status{SnowballTerms{}.touch_status};
    double principal_ratio{SnowballTerms{}.principal_ratio};
};

[[nodiscard]] inline Result<SnowballOption> make_otm_snowball(
    OutOfTheMoneySnowballTerms terms)
{
    return make_snowball_option({.knock_out_coupon_rates =
                                     std::vector<double>(terms.observation_dates.size(), terms.coupon_rate),
                                 .maturity_coupon_rate = terms.coupon_rate,
                                 .initial_spot = terms.initial_spot,
                                 .knock_in_level = terms.knock_in_level,
                                 .knock_out_levels =
                                     std::vector<double>(terms.observation_dates.size(), terms.knock_out_level),
                                 .upper_strike = terms.upper_strike,
                                 .lower_strike = 0.0,
                                 .observation_dates = std::move(terms.observation_dates),
                                 .knock_in_observation_mode = KnockInObservationMode::every_trading_day,
                                 .touch_status = terms.touch_status,
                                 .principal_ratio = terms.principal_ratio,
                                 .effective_date = terms.effective_date,
                                 .expiry_date = terms.expiry_date});
}

struct LossCappedSnowballTerms {
    double coupon_rate{};
    double initial_spot{};
    double knock_in_level{};
    double knock_out_level{};
    double lower_strike{};
    std::vector<Date> observation_dates;
    Date effective_date{};
    Date expiry_date{};
    BarrierTouchStatus touch_status{SnowballTerms{}.touch_status};
    double principal_ratio{SnowballTerms{}.principal_ratio};
};

[[nodiscard]] inline Result<SnowballOption> make_loss_capped_snowball(
    LossCappedSnowballTerms terms)
{
    return make_snowball_option({.knock_out_coupon_rates =
                                     std::vector<double>(terms.observation_dates.size(), terms.coupon_rate),
                                 .maturity_coupon_rate = terms.coupon_rate,
                                 .initial_spot = terms.initial_spot,
                                 .knock_in_level = terms.knock_in_level,
                                 .knock_out_levels =
                                     std::vector<double>(terms.observation_dates.size(), terms.knock_out_level),
                                 .upper_strike = terms.initial_spot,
                                 .lower_strike = terms.lower_strike,
                                 .observation_dates = std::move(terms.observation_dates),
                                 .knock_in_observation_mode = KnockInObservationMode::every_trading_day,
                                 .touch_status = terms.touch_status,
                                 .principal_ratio = terms.principal_ratio,
                                 .effective_date = terms.effective_date,
                                 .expiry_date = terms.expiry_date});
}

struct EuropeanSnowballTerms {
    double coupon_rate{};
    double initial_spot{};
    double knock_in_level{};
    double knock_out_level{};
    std::vector<Date> observation_dates;
    Date effective_date{};
    Date expiry_date{};
    BarrierTouchStatus touch_status{SnowballTerms{}.touch_status};
    double principal_ratio{SnowballTerms{}.principal_ratio};
};

[[nodiscard]] inline Result<SnowballOption> make_european_snowball(
    EuropeanSnowballTerms terms)
{
    return make_snowball_option({.knock_out_coupon_rates =
                                     std::vector<double>(terms.observation_dates.size(), terms.coupon_rate),
                                 .maturity_coupon_rate = terms.coupon_rate,
                                 .initial_spot = terms.initial_spot,
                                 .knock_in_level = terms.knock_in_level,
                                 .knock_out_levels =
                                     std::vector<double>(terms.observation_dates.size(), terms.knock_out_level),
                                 .upper_strike = terms.initial_spot,
                                 .lower_strike = 0.0,
                                 .observation_dates = std::move(terms.observation_dates),
                                 .knock_in_observation_mode = KnockInObservationMode::at_expiry,
                                 .touch_status = terms.touch_status,
                                 .principal_ratio = terms.principal_ratio,
                                 .effective_date = terms.effective_date,
                                 .expiry_date = terms.expiry_date});
}

} // namespace kiyosi
