#pragma once

#include <utility>
#include <vector>

#include <kiyosi/instruments/structured/snowball.hpp>

namespace kiyosi {

/// Named market conventions layered over make_snowball_option; each one only shapes the
/// coupon and knock-out ladders before delegating to the authoritative factory.

/// Terms for a constant-coupon, constant-knock-out snowball preset.
struct StandardSnowballTerms {
    double coupon_rate{};                                                  ///< Coupon rate used at knock-out and maturity.
    double initial_spot{};                                                 ///< Positive reference spot and upper strike.
    double knock_in_level{};                                               ///< Positive downside knock-in level.
    double knock_out_level{};                                              ///< Positive knock-out level used at every observation.
    std::vector<Date> observation_dates;                                   ///< Strictly ordered event dates.
    Date effective_date{};                                                 ///< First date of the note life.
    Date expiry_date{};                                                    ///< Final date of the note life.
    AutocallableBarrierState barrier_state{SnowballTerms{}.barrier_state}; ///< Prior barrier state.
    double principal_ratio{SnowballTerms{}.principal_ratio};               ///< Non-negative principal multiplier.
};

/// Creates a standard snowball from named market terms.
/// @return The option, or an `invalid_parameter` or `invalid_schedule` error.
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
                                 .barrier_state = terms.barrier_state,
                                 .principal_ratio = terms.principal_ratio,
                                 .effective_date = terms.effective_date,
                                 .expiry_date = terms.expiry_date});
}

/// Terms for a snowball whose knock-out level decreases by observation.
struct StepDownSnowballTerms {
    double coupon_rate{};             ///< Coupon rate used at knock-out and maturity.
    double initial_spot{};            ///< Positive reference spot and upper strike.
    double knock_in_level{};          ///< Positive downside knock-in level.
    double initial_knock_out_level{}; ///< First positive knock-out level.
    /// Absolute underlying-price decrement applied at each observation.
    double knock_out_level_decrement{};
    std::vector<Date> observation_dates;                                   ///< Strictly ordered event dates.
    Date effective_date{};                                                 ///< First date of the note life.
    Date expiry_date{};                                                    ///< Final date of the note life.
    AutocallableBarrierState barrier_state{SnowballTerms{}.barrier_state}; ///< Prior barrier state.
    double principal_ratio{SnowballTerms{}.principal_ratio};               ///< Non-negative principal multiplier.
};

/// Creates a step-down snowball from named market terms.
/// @return The option, or an `invalid_parameter` or `invalid_schedule` error.
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
                                 .barrier_state = terms.barrier_state,
                                 .principal_ratio = terms.principal_ratio,
                                 .effective_date = terms.effective_date,
                                 .expiry_date = terms.expiry_date});
}

/// Terms for a snowball whose coupon and knock-out level both decrease by observation.
struct BothDownSnowballTerms {
    double initial_coupon_rate{}; ///< Coupon rate at the first observation.
    /// Absolute decimal-rate decrement applied at each observation (0.01 is one percentage point).
    double coupon_rate_decrement{};
    double initial_spot{};            ///< Positive reference spot and upper strike.
    double knock_in_level{};          ///< Positive downside knock-in level.
    double initial_knock_out_level{}; ///< First positive knock-out level.
    /// Absolute underlying-price decrement applied at each observation.
    double knock_out_level_decrement{};
    std::vector<Date> observation_dates;                                   ///< Strictly ordered event dates.
    Date effective_date{};                                                 ///< First date of the note life.
    Date expiry_date{};                                                    ///< Final date of the note life.
    AutocallableBarrierState barrier_state{SnowballTerms{}.barrier_state}; ///< Prior barrier state.
    double principal_ratio{SnowballTerms{}.principal_ratio};               ///< Non-negative principal multiplier.
};

/// Creates a both-down snowball from named market terms.
/// @return The option, or an `invalid_parameter` or `invalid_schedule` error.
[[nodiscard]] inline Result<SnowballOption> make_both_down_snowball(
    BothDownSnowballTerms terms)
{
    std::vector<double> coupons;
    std::vector<double> knock_out_levels;
    coupons.reserve(terms.observation_dates.size());
    knock_out_levels.reserve(terms.observation_dates.size());
    for (std::size_t index = 0; index < terms.observation_dates.size(); ++index) {
        coupons.push_back(terms.initial_coupon_rate - static_cast<double>(index) * terms.coupon_rate_decrement);
        knock_out_levels.push_back(terms.initial_knock_out_level - static_cast<double>(index) * terms.knock_out_level_decrement);
    }
    const double maturity_coupon_rate = coupons.empty() ? 0.0 : coupons.back();
    return make_snowball_option({.knock_out_coupon_rates = std::move(coupons),
                                 .maturity_coupon_rate = maturity_coupon_rate,
                                 .initial_spot = terms.initial_spot,
                                 .knock_in_level = terms.knock_in_level,
                                 .knock_out_levels = std::move(knock_out_levels),
                                 .upper_strike = terms.initial_spot,
                                 .lower_strike = 0.0,
                                 .observation_dates = std::move(terms.observation_dates),
                                 .knock_in_observation_mode = KnockInObservationMode::every_trading_day,
                                 .barrier_state = terms.barrier_state,
                                 .principal_ratio = terms.principal_ratio,
                                 .effective_date = terms.effective_date,
                                 .expiry_date = terms.expiry_date});
}

/// Terms for a snowball with distinct knock-out and maturity coupons.
struct DualCouponSnowballTerms {
    double knock_out_coupon_rate{};                                        ///< Coupon rate paid after knock-out.
    double maturity_coupon_rate{};                                         ///< Coupon rate paid at maturity when applicable.
    double initial_spot{};                                                 ///< Positive reference spot and upper strike.
    double knock_in_level{};                                               ///< Positive downside knock-in level.
    double knock_out_level{};                                              ///< Positive knock-out level used at every observation.
    std::vector<Date> observation_dates;                                   ///< Strictly ordered event dates.
    Date effective_date{};                                                 ///< First date of the note life.
    Date expiry_date{};                                                    ///< Final date of the note life.
    AutocallableBarrierState barrier_state{SnowballTerms{}.barrier_state}; ///< Prior barrier state.
    double principal_ratio{SnowballTerms{}.principal_ratio};               ///< Non-negative principal multiplier.
};

/// Creates a dual-coupon snowball from named market terms.
/// @return The option, or an `invalid_parameter` or `invalid_schedule` error.
[[nodiscard]] inline Result<SnowballOption> make_dual_coupon_snowball(
    DualCouponSnowballTerms terms)
{
    return make_snowball_option({.knock_out_coupon_rates =
                                     std::vector<double>(terms.observation_dates.size(), terms.knock_out_coupon_rate),
                                 .maturity_coupon_rate = terms.maturity_coupon_rate,
                                 .initial_spot = terms.initial_spot,
                                 .knock_in_level = terms.knock_in_level,
                                 .knock_out_levels =
                                     std::vector<double>(terms.observation_dates.size(), terms.knock_out_level),
                                 .upper_strike = terms.initial_spot,
                                 .lower_strike = 0.0,
                                 .observation_dates = std::move(terms.observation_dates),
                                 .knock_in_observation_mode = KnockInObservationMode::every_trading_day,
                                 .barrier_state = terms.barrier_state,
                                 .principal_ratio = terms.principal_ratio,
                                 .effective_date = terms.effective_date,
                                 .expiry_date = terms.expiry_date});
}

/// Terms for a snowball with a distinct final knock-out level.
struct ParachuteSnowballTerms {
    double coupon_rate{};                                                  ///< Coupon rate used at knock-out and maturity.
    double initial_spot{};                                                 ///< Positive reference spot and upper strike.
    double knock_in_level{};                                               ///< Positive downside knock-in level.
    double knock_out_level{};                                              ///< Positive knock-out level before the final observation.
    double final_knock_out_level{};                                        ///< Positive knock-out level at the final observation.
    std::vector<Date> observation_dates;                                   ///< Strictly ordered event dates.
    Date effective_date{};                                                 ///< First date of the note life.
    Date expiry_date{};                                                    ///< Final date of the note life.
    AutocallableBarrierState barrier_state{SnowballTerms{}.barrier_state}; ///< Prior barrier state.
    double principal_ratio{SnowballTerms{}.principal_ratio};               ///< Non-negative principal multiplier.
};

/// Creates a parachute snowball from named market terms.
/// @return The option, or an `invalid_parameter` or `invalid_schedule` error.
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
                                 .barrier_state = terms.barrier_state,
                                 .principal_ratio = terms.principal_ratio,
                                 .effective_date = terms.effective_date,
                                 .expiry_date = terms.expiry_date});
}

/// Terms for a snowball with an out-of-the-money upper settlement strike.
struct OtmSnowballTerms {
    double coupon_rate{};                                                  ///< Coupon rate used at knock-out and maturity.
    double initial_spot{};                                                 ///< Positive reference spot.
    double knock_in_level{};                                               ///< Positive downside knock-in level.
    double knock_out_level{};                                              ///< Positive knock-out level used at every observation.
    double upper_strike{};                                                 ///< Positive upper settlement strike.
    std::vector<Date> observation_dates;                                   ///< Strictly ordered event dates.
    Date effective_date{};                                                 ///< First date of the note life.
    Date expiry_date{};                                                    ///< Final date of the note life.
    AutocallableBarrierState barrier_state{SnowballTerms{}.barrier_state}; ///< Prior barrier state.
    double principal_ratio{SnowballTerms{}.principal_ratio};               ///< Non-negative principal multiplier.
};

/// Creates an out-of-the-money snowball from named market terms.
/// @return The option, or an `invalid_parameter` or `invalid_schedule` error.
[[nodiscard]] inline Result<SnowballOption> make_otm_snowball(
    OtmSnowballTerms terms)
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
                                 .barrier_state = terms.barrier_state,
                                 .principal_ratio = terms.principal_ratio,
                                 .effective_date = terms.effective_date,
                                 .expiry_date = terms.expiry_date});
}

/// Terms for a snowball whose downside participation is capped by a lower strike.
struct LossCappedSnowballTerms {
    double coupon_rate{};                                                  ///< Coupon rate used at knock-out and maturity.
    double initial_spot{};                                                 ///< Positive reference spot and upper strike.
    double knock_in_level{};                                               ///< Positive downside knock-in level.
    double knock_out_level{};                                              ///< Positive knock-out level used at every observation.
    double lower_strike{};                                                 ///< Non-negative downside settlement floor.
    std::vector<Date> observation_dates;                                   ///< Strictly ordered event dates.
    Date effective_date{};                                                 ///< First date of the note life.
    Date expiry_date{};                                                    ///< Final date of the note life.
    AutocallableBarrierState barrier_state{SnowballTerms{}.barrier_state}; ///< Prior barrier state.
    double principal_ratio{SnowballTerms{}.principal_ratio};               ///< Non-negative principal multiplier.
};

/// Creates a loss-capped snowball from named market terms.
/// @return The option, or an `invalid_parameter` or `invalid_schedule` error.
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
                                 .barrier_state = terms.barrier_state,
                                 .principal_ratio = terms.principal_ratio,
                                 .effective_date = terms.effective_date,
                                 .expiry_date = terms.expiry_date});
}

/// Terms for a snowball whose knock-in barrier is observed only at expiry.
struct EuropeanSnowballTerms {
    double coupon_rate{};                                                  ///< Coupon rate used at knock-out and maturity.
    double initial_spot{};                                                 ///< Positive reference spot and upper strike.
    double knock_in_level{};                                               ///< Positive downside knock-in level.
    double knock_out_level{};                                              ///< Positive knock-out level used at every observation.
    std::vector<Date> observation_dates;                                   ///< Strictly ordered event dates.
    Date effective_date{};                                                 ///< First date of the note life.
    Date expiry_date{};                                                    ///< Final date of the note life.
    AutocallableBarrierState barrier_state{SnowballTerms{}.barrier_state}; ///< Prior barrier state.
    double principal_ratio{SnowballTerms{}.principal_ratio};               ///< Non-negative principal multiplier.
};

/// Creates a European-knock-in snowball from named market terms.
/// @return The option, or an `invalid_parameter` or `invalid_schedule` error.
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
                                 .barrier_state = terms.barrier_state,
                                 .principal_ratio = terms.principal_ratio,
                                 .effective_date = terms.effective_date,
                                 .expiry_date = terms.expiry_date});
}

} // namespace kiyosi
