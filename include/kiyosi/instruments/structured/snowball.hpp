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
    double initial_spot{};
    double knock_in_level{};
    std::vector<double> knock_out_levels;
    double upper_strike{};
    double lower_strike{};
    std::vector<Date> observation_dates;
    KnockInObservationMode knock_in_observation_mode{};
    BarrierTouchStatus touch_status{BarrierTouchStatus::none};
    double principal_ratio{1.0};
    Date effective_date{};
    Date expiry_date{};
};

struct TernarySnowballTerms {
    std::vector<double> knock_out_coupon_rates;
    double maturity_coupon_rate{};
    double minimum_coupon_rate{};
    double initial_spot{};
    double knock_in_level{};
    std::vector<double> knock_out_levels;
    double upper_strike{};
    double lower_strike{};
    std::vector<Date> observation_dates;
    KnockInObservationMode knock_in_observation_mode{};
    BarrierTouchStatus touch_status{BarrierTouchStatus::none};
    double principal_ratio{1.0};
    Date effective_date{};
    Date expiry_date{};
};

struct BinarySnowballTerms {
    std::vector<double> knock_out_coupon_rates;
    double maturity_coupon_rate{};
    double initial_spot{};
    std::vector<double> knock_out_levels;
    double upper_strike{};
    double lower_strike{};
    std::vector<Date> observation_dates;
    BarrierTouchStatus touch_status{BarrierTouchStatus::none};
    double principal_ratio{1.0};
    Date effective_date{};
    Date expiry_date{};
};

[[nodiscard]] Result<SnowballOption> make_snowball_option(SnowballTerms);
[[nodiscard]] Result<TernarySnowballOption> make_ternary_snowball_option(TernarySnowballTerms);

[[nodiscard]] Result<BinarySnowballOption> make_binary_snowball_option(BinarySnowballTerms);

/// Knock-in autocallable accruing a coupon until knock-out, with downside participation.
class SnowballOption : public KnockInAutocallableNote {
public:
    const std::vector<double>& knock_out_coupon_rates() const noexcept { return knock_out_coupon_rates_; }
    double maturity_coupon_rate() const noexcept { return maturity_coupon_rate_; }

    friend bool operator==(const SnowballOption&, const SnowballOption&) = default;

private:
    SnowballOption(std::vector<double> knock_out_coupon_rates, double maturity_coupon_rate,
                   double initial_spot, double knock_in_level, std::vector<double> knock_out_levels,
                   double upper_strike, double lower_strike, std::vector<Date> observation_dates,
                   KnockInObservationMode knock_in_observation_mode, BarrierTouchStatus touch_status,
                   double principal_ratio, Date effective_date, Date expiry_date)
        : KnockInAutocallableNote(initial_spot, knock_in_level, std::move(knock_out_levels), upper_strike,
                             lower_strike, std::move(observation_dates), knock_in_observation_mode, touch_status,
                             principal_ratio, effective_date, expiry_date),
          knock_out_coupon_rates_(std::move(knock_out_coupon_rates)),
          maturity_coupon_rate_(maturity_coupon_rate) {}

    std::vector<double> knock_out_coupon_rates_;
    double maturity_coupon_rate_;

    friend Result<SnowballOption> make_snowball_option(SnowballTerms);
};

/// Snowball variant settling a flat coupon at maturity regardless of the terminal spot.
class BinarySnowballOption : public AutocallableNote {
public:
    const std::vector<double>& knock_out_coupon_rates() const noexcept { return knock_out_coupon_rates_; }
    double maturity_coupon_rate() const noexcept { return maturity_coupon_rate_; }
    friend bool operator==(const BinarySnowballOption&, const BinarySnowballOption&) = default;

private:
    BinarySnowballOption(std::vector<double> knock_out_coupon_rates, double maturity_coupon_rate,
                         double initial_spot, std::vector<double> knock_out_levels,
                         double upper_strike, double lower_strike, std::vector<Date> observation_dates,
                         BarrierTouchStatus touch_status, double principal_ratio, Date effective_date,
                         Date expiry_date)
        : AutocallableNote(initial_spot, std::move(knock_out_levels), upper_strike, lower_strike,
                           std::move(observation_dates), principal_ratio, touch_status, effective_date, expiry_date),
          knock_out_coupon_rates_(std::move(knock_out_coupon_rates)),
          maturity_coupon_rate_(maturity_coupon_rate) {}

    std::vector<double> knock_out_coupon_rates_;
    double maturity_coupon_rate_;

    friend Result<BinarySnowballOption> make_binary_snowball_option(BinarySnowballTerms);
};

/// Snowball variant whose maturity coupon steps down to a floor once knocked in.
class TernarySnowballOption : public KnockInAutocallableNote {
public:
    const std::vector<double>& knock_out_coupon_rates() const noexcept { return knock_out_coupon_rates_; }
    double maturity_coupon_rate() const noexcept { return maturity_coupon_rate_; }
    double minimum_coupon_rate() const noexcept { return minimal_coupon_rate_; }
    friend bool operator==(const TernarySnowballOption&, const TernarySnowballOption&) = default;

private:
    TernarySnowballOption(std::vector<double> knock_out_coupon_rates, double maturity_coupon_rate,
                          double minimum_coupon_rate, double initial_spot, double knock_in_level,
                          std::vector<double> knock_out_levels, double upper_strike,
                          double lower_strike, std::vector<Date> observation_dates,
                          KnockInObservationMode knock_in_observation_mode, BarrierTouchStatus touch_status,
                          double principal_ratio, Date effective_date, Date expiry_date)
        : KnockInAutocallableNote(initial_spot, knock_in_level, std::move(knock_out_levels), upper_strike,
                             lower_strike, std::move(observation_dates), knock_in_observation_mode, touch_status,
                             principal_ratio, effective_date, expiry_date),
          knock_out_coupon_rates_(std::move(knock_out_coupon_rates)),
          maturity_coupon_rate_(maturity_coupon_rate), minimal_coupon_rate_(minimum_coupon_rate) {}

    std::vector<double> knock_out_coupon_rates_;
    double maturity_coupon_rate_;
    double minimal_coupon_rate_;

    friend Result<TernarySnowballOption> make_ternary_snowball_option(TernarySnowballTerms);
};

[[nodiscard]] inline Result<BinarySnowballOption> make_binary_snowball_option(BinarySnowballTerms terms)
{
    return detail::validate_and_return_autocallable_note(BinarySnowballOption{std::move(terms.knock_out_coupon_rates), terms.maturity_coupon_rate,
                                               terms.initial_spot, std::move(terms.knock_out_levels), terms.upper_strike,
                                               terms.lower_strike, std::move(terms.observation_dates), terms.touch_status,
                                               terms.principal_ratio, terms.effective_date, terms.expiry_date});
}

[[nodiscard]] inline Result<SnowballOption> make_snowball_option(SnowballTerms terms)
{
    return detail::validate_and_return_autocallable_note(SnowballOption{std::move(terms.knock_out_coupon_rates), terms.maturity_coupon_rate,
                                         terms.initial_spot, terms.knock_in_level,
                                         std::move(terms.knock_out_levels), terms.upper_strike, terms.lower_strike,
                                         std::move(terms.observation_dates), terms.knock_in_observation_mode, terms.touch_status,
                                         terms.principal_ratio, terms.effective_date, terms.expiry_date});
}

[[nodiscard]] inline Result<TernarySnowballOption> make_ternary_snowball_option(TernarySnowballTerms terms)
{
    return detail::validate_and_return_autocallable_note(TernarySnowballOption{std::move(terms.knock_out_coupon_rates), terms.maturity_coupon_rate,
                                                terms.minimum_coupon_rate, terms.initial_spot, terms.knock_in_level,
                                                std::move(terms.knock_out_levels), terms.upper_strike, terms.lower_strike,
                                                std::move(terms.observation_dates), terms.knock_in_observation_mode, terms.touch_status,
                                                terms.principal_ratio, terms.effective_date, terms.expiry_date});
}

} // namespace kiyosi
