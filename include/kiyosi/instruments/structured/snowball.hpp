#pragma once

#include <utility>
#include <vector>

#include <kiyosi/instruments/structured/autocallable.hpp>

namespace kiyosi {

class SnowballOption;
class BinarySnowballOption;
class TernarySnowballOption;

struct SnowballTerms {
    std::vector<double> knock_out_coupon_rates;
    double maturity_coupon_rate{};
    double initial_price{};
    double knock_in_price{};
    std::vector<double> knock_out_prices;
    double upper_strike{};
    double lower_strike{};
    std::vector<date> observations;
    observation_frequency frequency{};
    barrier_touch_status touch_status{};
    double principal_ratio{};
    date effective{};
    date expiry{};
};

struct TernarySnowballTerms {
    std::vector<double> knock_out_coupon_rates;
    double maturity_coupon_rate{};
    double minimal_coupon_rate{};
    double initial_price{};
    double knock_in_price{};
    std::vector<double> knock_out_prices;
    double upper_strike{};
    double lower_strike{};
    std::vector<date> observations;
    observation_frequency frequency{};
    barrier_touch_status touch_status{};
    double principal_ratio{};
    date effective{};
    date expiry{};
};

struct BinarySnowballTerms {
    std::vector<double> knock_out_coupon_rates;
    double maturity_coupon_rate{};
    double initial_price{};
    std::vector<double> knock_out_prices;
    double upper_strike{};
    double lower_strike{};
    std::vector<date> observations;
    barrier_touch_status touch_status{};
    double principal_ratio{};
    date effective{};
    date expiry{};
};

[[nodiscard]] result<SnowballOption> make_snowball_option(SnowballTerms);
[[nodiscard]] result<TernarySnowballOption> make_ternary_snowball_option(TernarySnowballTerms);

[[nodiscard]] result<BinarySnowballOption> make_binary_snowball_option(BinarySnowballTerms);

/// Knock-in autocallable accruing a coupon until knock-out, with downside participation.
class SnowballOption : public KiAutocallableNote {
public:
    const std::vector<double>& knock_out_coupon_rates() const noexcept { return knock_out_coupon_rates_; }
    double maturity_coupon_rate() const noexcept { return maturity_coupon_rate_; }

    result<SnowballOption> with_coupon_rate(double coupon) const;
    friend bool operator==(const SnowballOption&, const SnowballOption&) = default;

private:
    SnowballOption(std::vector<double> knock_out_coupon_rates, double maturity_coupon_rate,
                   double initial_price, double knock_in_price, std::vector<double> knock_out_prices,
                   double upper_strike, double lower_strike, std::vector<date> observations,
                   observation_frequency frequency, barrier_touch_status touch_status,
                   double principal_ratio, date effective, date expiry)
        : KiAutocallableNote(initial_price, knock_in_price, std::move(knock_out_prices), upper_strike,
                             lower_strike, std::move(observations), frequency, touch_status,
                             principal_ratio, effective, expiry),
          knock_out_coupon_rates_(std::move(knock_out_coupon_rates)),
          maturity_coupon_rate_(maturity_coupon_rate) {}

    std::vector<double> knock_out_coupon_rates_;
    double maturity_coupon_rate_;

    friend result<SnowballOption> make_snowball_option(SnowballTerms);
};

/// Snowball variant settling a flat coupon at maturity regardless of the terminal spot.
class BinarySnowballOption : public AutocallableNote {
public:
    const std::vector<double>& knock_out_coupon_rates() const noexcept { return knock_out_coupon_rates_; }
    double maturity_coupon_rate() const noexcept { return maturity_coupon_rate_; }
    friend bool operator==(const BinarySnowballOption&, const BinarySnowballOption&) = default;

private:
    BinarySnowballOption(std::vector<double> knock_out_coupon_rates, double maturity_coupon_rate,
                         double initial_price, std::vector<double> knock_out_prices,
                         double upper_strike, double lower_strike, std::vector<date> observations,
                         barrier_touch_status touch_status, double principal_ratio, date effective,
                         date expiry)
        : AutocallableNote(initial_price, std::move(knock_out_prices), upper_strike, lower_strike,
                           std::move(observations), principal_ratio, touch_status, effective, expiry),
          knock_out_coupon_rates_(std::move(knock_out_coupon_rates)),
          maturity_coupon_rate_(maturity_coupon_rate) {}

    std::vector<double> knock_out_coupon_rates_;
    double maturity_coupon_rate_;

    friend result<BinarySnowballOption> make_binary_snowball_option(BinarySnowballTerms);
};

/// Snowball variant whose maturity coupon steps down to a floor once knocked in.
class TernarySnowballOption : public KiAutocallableNote {
public:
    const std::vector<double>& knock_out_coupon_rates() const noexcept { return knock_out_coupon_rates_; }
    double maturity_coupon_rate() const noexcept { return maturity_coupon_rate_; }
    double minimal_coupon_rate() const noexcept { return minimal_coupon_rate_; }
    friend bool operator==(const TernarySnowballOption&, const TernarySnowballOption&) = default;

private:
    TernarySnowballOption(std::vector<double> knock_out_coupon_rates, double maturity_coupon_rate,
                          double minimal_coupon_rate, double initial_price, double knock_in_price,
                          std::vector<double> knock_out_prices, double upper_strike,
                          double lower_strike, std::vector<date> observations,
                          observation_frequency frequency, barrier_touch_status touch_status,
                          double principal_ratio, date effective, date expiry)
        : KiAutocallableNote(initial_price, knock_in_price, std::move(knock_out_prices), upper_strike,
                             lower_strike, std::move(observations), frequency, touch_status,
                             principal_ratio, effective, expiry),
          knock_out_coupon_rates_(std::move(knock_out_coupon_rates)),
          maturity_coupon_rate_(maturity_coupon_rate), minimal_coupon_rate_(minimal_coupon_rate) {}

    std::vector<double> knock_out_coupon_rates_;
    double maturity_coupon_rate_;
    double minimal_coupon_rate_;

    friend result<TernarySnowballOption> make_ternary_snowball_option(TernarySnowballTerms);
};

[[nodiscard]] inline result<BinarySnowballOption> make_binary_snowball_option(BinarySnowballTerms terms)
{
    return validate_note(BinarySnowballOption{std::move(terms.knock_out_coupon_rates), terms.maturity_coupon_rate,
                                              terms.initial_price, std::move(terms.knock_out_prices), terms.upper_strike,
                                              terms.lower_strike, std::move(terms.observations), terms.touch_status,
                                              terms.principal_ratio, terms.effective, terms.expiry});
}

inline result<SnowballOption> SnowballOption::with_coupon_rate(double coupon) const
{
    return make_snowball_option({knock_out_coupon_rates(), coupon, initial_price(), knock_in_price(),
                                 knock_out_prices(), upper_strike(), lower_strike(), observation_dates(),
                                 knock_in_frequency(), touch_status(), principal_ratio(), effective(), expiry()});
}

[[nodiscard]] inline result<SnowballOption> make_snowball_option(SnowballTerms terms)
{
    return validate_note(SnowballOption{std::move(terms.knock_out_coupon_rates), terms.maturity_coupon_rate,
                                        terms.initial_price, terms.knock_in_price,
                                        std::move(terms.knock_out_prices), terms.upper_strike, terms.lower_strike,
                                        std::move(terms.observations), terms.frequency, terms.touch_status,
                                        terms.principal_ratio, terms.effective, terms.expiry});
}

[[nodiscard]] inline result<TernarySnowballOption> make_ternary_snowball_option(TernarySnowballTerms terms)
{
    return validate_note(TernarySnowballOption{std::move(terms.knock_out_coupon_rates), terms.maturity_coupon_rate,
                                               terms.minimal_coupon_rate, terms.initial_price, terms.knock_in_price,
                                               std::move(terms.knock_out_prices), terms.upper_strike, terms.lower_strike,
                                               std::move(terms.observations), terms.frequency, terms.touch_status,
                                               terms.principal_ratio, terms.effective, terms.expiry});
}

} // namespace kiyosi
