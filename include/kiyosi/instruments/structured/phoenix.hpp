#pragma once

#include <utility>
#include <vector>

#include <kiyosi/instruments/structured/autocallable.hpp>

namespace kiyosi {

class PhoenixOption;

struct PhoenixTerms {
    double coupon_rate{};
    double initial_spot{};
    double knock_in_level{};
    std::vector<double> knock_out_levels;
    std::vector<double> coupon_barrier_levels;
    double upper_strike{};
    double lower_strike{};
    std::vector<Date> observation_dates;
    KnockInObservationMode knock_in_observation_mode{};
    BarrierTouchStatus touch_status{BarrierTouchStatus::none};
    double principal_ratio{1.0};
    Date effective_date{};
    Date expiry_date{};
};

[[nodiscard]] Result<PhoenixOption> make_phoenix_option(PhoenixTerms);

/// Knock-in autocallable paying a conditional coupon whenever spot clears the coupon barrier.
class PhoenixOption : public KnockInAutocallableNote {
public:
    double coupon_rate() const noexcept { return coupon_rate_; }
    const std::vector<double>& coupon_barrier_levels() const noexcept { return coupon_barriers_; }

    friend bool operator==(const PhoenixOption&, const PhoenixOption&) = default;

private:
    PhoenixOption(double coupon_rate, double initial_spot, double knock_in_level,
                  std::vector<double> knock_out_levels, std::vector<double> coupon_barrier_levels,
                  double upper_strike, double lower_strike, std::vector<Date> observation_dates,
                  KnockInObservationMode knock_in_observation_mode, BarrierTouchStatus touch_status,
                  double principal_ratio, Date effective_date, Date expiry_date)
        : KnockInAutocallableNote(initial_spot, knock_in_level, std::move(knock_out_levels), upper_strike,
                             lower_strike, std::move(observation_dates), knock_in_observation_mode, touch_status,
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
                                        terms.knock_in_observation_mode, terms.touch_status, terms.principal_ratio,
                                        terms.effective_date, terms.expiry_date});
}

} // namespace kiyosi
