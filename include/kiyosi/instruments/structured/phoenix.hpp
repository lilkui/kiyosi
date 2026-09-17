#pragma once

#include <utility>
#include <vector>

#include <kiyosi/instruments/structured/autocallable.hpp>

namespace kiyosi {

class PhoenixOption;

struct PhoenixTerms {
    double coupon_rate{};
    double initial_price{};
    double knock_in_price{};
    std::vector<double> knock_out_prices;
    std::vector<double> coupon_barriers;
    double upper_strike{};
    double lower_strike{};
    std::vector<date> observation_dates;
    observation_frequency frequency{};
    barrier_touch_status touch_status{barrier_touch_status::none};
    double principal_ratio{1.0};
    date effective{};
    date expiry{};
};

[[nodiscard]] result<PhoenixOption> make_phoenix_option(PhoenixTerms);

/// Knock-in autocallable paying a conditional coupon whenever spot clears the coupon barrier.
class PhoenixOption : public KiAutocallableNote {
public:
    double coupon_rate() const noexcept { return coupon_rate_; }
    const std::vector<double>& coupon_barriers() const noexcept { return coupon_barriers_; }

    result<PhoenixOption> with_coupon_rate(double coupon) const;
    friend bool operator==(const PhoenixOption&, const PhoenixOption&) = default;

private:
    PhoenixOption(double coupon_rate, double initial_price, double knock_in_price,
                  std::vector<double> knock_out_prices, std::vector<double> coupon_barriers,
                  double upper_strike, double lower_strike, std::vector<date> observation_dates,
                  observation_frequency frequency, barrier_touch_status touch_status,
                  double principal_ratio, date effective, date expiry)
        : KiAutocallableNote(initial_price, knock_in_price, std::move(knock_out_prices), upper_strike,
                             lower_strike, std::move(observation_dates), frequency, touch_status,
                             principal_ratio, effective, expiry),
          coupon_rate_(coupon_rate), coupon_barriers_(std::move(coupon_barriers)) {}

    double coupon_rate_;
    std::vector<double> coupon_barriers_;

    friend result<PhoenixOption> make_phoenix_option(PhoenixTerms);
};

inline result<PhoenixOption> PhoenixOption::with_coupon_rate(double coupon) const
{
    return make_phoenix_option(PhoenixTerms{.coupon_rate = coupon,
                                            .initial_price = initial_price(),
                                            .knock_in_price = knock_in_price(),
                                            .knock_out_prices = knock_out_prices(),
                                            .coupon_barriers = coupon_barriers(),
                                            .upper_strike = upper_strike(),
                                            .lower_strike = lower_strike(),
                                            .observation_dates = observation_dates(),
                                            .frequency = knock_in_frequency(),
                                            .touch_status = touch_status(),
                                            .principal_ratio = principal_ratio(),
                                            .effective = effective(),
                                            .expiry = expiry()});
}

[[nodiscard]] inline result<PhoenixOption> make_phoenix_option(PhoenixTerms terms)
{
    return validate_note(PhoenixOption{terms.coupon_rate, terms.initial_price, terms.knock_in_price,
                                       std::move(terms.knock_out_prices), std::move(terms.coupon_barriers),
                                       terms.upper_strike, terms.lower_strike, std::move(terms.observation_dates),
                                       terms.frequency, terms.touch_status, terms.principal_ratio,
                                       terms.effective, terms.expiry});
}

} // namespace kiyosi
