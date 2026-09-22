#pragma once

#include <utility>
#include <vector>

#include <kiyosi/instruments/structured/autocallable.hpp>

namespace kiyosi {

class PhoenixOption;

/// Input terms used to construct a PhoenixOption.
struct PhoenixTerms {
    double coupon_rate{};                                                   ///< Finite coupon rate paid at qualifying observations.
    double initial_spot{};                                                  ///< Positive reference spot.
    double knock_in_level{};                                                ///< Positive downside knock-in level.
    std::vector<double> knock_out_levels;                                   ///< Positive knock-out levels by observation.
    std::vector<double> coupon_barrier_levels;                              ///< Non-negative coupon barriers by observation.
    double upper_strike{};                                                  ///< Positive upper settlement strike.
    double lower_strike{};                                                  ///< Non-negative lower settlement strike.
    std::vector<Date> observation_dates;                                    ///< Strictly ordered event dates.
    KnockInObservationMode knock_in_observation_mode{};                     ///< Knock-in monitoring frequency.
    AutocallableBarrierState barrier_state{AutocallableBarrierState::none}; ///< Prior barrier state.
    double principal_ratio{1.0};                                            ///< Non-negative principal multiplier.
    Date effective_date{};                                                  ///< First date of the note life.
    Date expiry_date{};                                                     ///< Final date of the note life.
};

/// Creates a validated Phoenix autocallable.
/// @return The option, or an `invalid_parameter` or `invalid_schedule` error.
[[nodiscard]] Result<PhoenixOption> make_phoenix_option(PhoenixTerms);

/// Knock-in autocallable paying a conditional coupon whenever spot clears the coupon barrier.
class PhoenixOption : public KnockInAutocallableNote {
public:
    /// Returns the finite conditional coupon rate.
    double coupon_rate() const noexcept { return coupon_rate_; }
    /// Returns one non-negative coupon barrier per observation date.
    const std::vector<double>& coupon_barrier_levels() const noexcept { return coupon_barriers_; }

    /// Compares all autocallable and coupon terms.
    friend bool operator==(const PhoenixOption&, const PhoenixOption&) = default;

private:
    PhoenixOption(double coupon_rate, double initial_spot, double knock_in_level,
                  std::vector<double> knock_out_levels, std::vector<double> coupon_barrier_levels,
                  double upper_strike, double lower_strike, std::vector<Date> observation_dates,
                  KnockInObservationMode knock_in_observation_mode, AutocallableBarrierState barrier_state,
                  double principal_ratio, Date effective_date, Date expiry_date)
        : KnockInAutocallableNote(initial_spot, knock_in_level, std::move(knock_out_levels), upper_strike,
                                  lower_strike, std::move(observation_dates), knock_in_observation_mode, barrier_state,
                                  principal_ratio, effective_date, expiry_date),
          coupon_rate_(coupon_rate), coupon_barriers_(std::move(coupon_barrier_levels)) {}

    double coupon_rate_;
    std::vector<double> coupon_barriers_;

    friend Result<PhoenixOption> make_phoenix_option(PhoenixTerms);
};

[[nodiscard]] inline Result<PhoenixOption> make_phoenix_option(PhoenixTerms terms)
{
    return detail::validate_and_return_autocallable_note(PhoenixOption{terms.coupon_rate, terms.initial_spot, terms.knock_in_level,
                                                                       std::move(terms.knock_out_levels), std::move(terms.coupon_barrier_levels),
                                                                       terms.upper_strike, terms.lower_strike, std::move(terms.observation_dates),
                                                                       terms.knock_in_observation_mode, terms.barrier_state, terms.principal_ratio,
                                                                       terms.effective_date, terms.expiry_date});
}

} // namespace kiyosi
