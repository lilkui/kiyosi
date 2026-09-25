#pragma once

#include <utility>
#include <vector>

#include <kiyosi/instruments/structured/autocallable.hpp>

namespace kiyosi {

class SnowballOption;
class BinarySnowballOption;
class TernarySnowballOption;

/// Input terms used to construct a SnowballOption.
struct SnowballTerms {
    std::vector<double> knock_out_coupon_rates;                             ///< Finite knock-out coupons by observation.
    double maturity_coupon_rate{};                                          ///< Finite coupon paid at maturity when applicable.
    double initial_spot{};                                                  ///< Positive reference spot.
    double knock_in_level{};                                                ///< Positive downside knock-in level.
    std::vector<double> knock_out_levels;                                   ///< Positive knock-out levels by observation.
    double upper_strike{};                                                  ///< Positive upper settlement strike.
    double lower_strike{};                                                  ///< Non-negative lower settlement strike.
    std::vector<Date> observation_dates;                                    ///< Strictly ordered event dates.
    KnockInObservationMode knock_in_observation_mode{};                     ///< Knock-in monitoring frequency.
    AutocallableBarrierState barrier_state{AutocallableBarrierState::none}; ///< Prior barrier state.
    double principal_ratio{1.0};                                            ///< Non-negative principal multiplier.
    Date effective_date{};                                                  ///< First date of the note life.
    Date expiry_date{};                                                     ///< Final date of the note life.
};

/// Input terms used to construct a TernarySnowballOption.
struct TernarySnowballTerms {
    std::vector<double> knock_out_coupon_rates;                             ///< Finite knock-out coupons by observation.
    double maturity_coupon_rate{};                                          ///< Finite coupon paid at maturity before knock-in adjustment.
    double minimum_coupon_rate{};                                           ///< Finite maturity-coupon floor after knock-in.
    double initial_spot{};                                                  ///< Positive reference spot.
    double knock_in_level{};                                                ///< Positive downside knock-in level.
    std::vector<double> knock_out_levels;                                   ///< Positive knock-out levels by observation.
    double upper_strike{};                                                  ///< Positive upper settlement strike.
    double lower_strike{};                                                  ///< Non-negative lower settlement strike.
    std::vector<Date> observation_dates;                                    ///< Strictly ordered event dates.
    KnockInObservationMode knock_in_observation_mode{};                     ///< Knock-in monitoring frequency.
    AutocallableBarrierState barrier_state{AutocallableBarrierState::none}; ///< Prior barrier state.
    double principal_ratio{1.0};                                            ///< Non-negative principal multiplier.
    Date effective_date{};                                                  ///< First date of the note life.
    Date expiry_date{};                                                     ///< Final date of the note life.
};

/// Input terms used to construct a BinarySnowballOption.
struct BinarySnowballTerms {
    std::vector<double> knock_out_coupon_rates;                             ///< Finite knock-out coupons by observation.
    double maturity_coupon_rate{};                                          ///< Finite flat maturity coupon.
    double initial_spot{};                                                  ///< Positive reference spot.
    std::vector<double> knock_out_levels;                                   ///< Positive knock-out levels by observation.
    double upper_strike{};                                                  ///< Positive upper settlement strike.
    double lower_strike{};                                                  ///< Non-negative lower settlement strike.
    std::vector<Date> observation_dates;                                    ///< Strictly ordered event dates.
    AutocallableBarrierState barrier_state{AutocallableBarrierState::none}; ///< Prior barrier state.
    double principal_ratio{1.0};                                            ///< Non-negative principal multiplier.
    Date effective_date{};                                                  ///< First date of the note life.
    Date expiry_date{};                                                     ///< Final date of the note life.
};

/// Creates a validated knock-in snowball.
/// @return The option, or an input-validation error with a stable category.
[[nodiscard]] Result<SnowballOption> make_snowball_option(SnowballTerms);
/// Creates a validated ternary snowball.
/// @return The option, or an input-validation error with a stable category.
[[nodiscard]] Result<TernarySnowballOption> make_ternary_snowball_option(TernarySnowballTerms);

/// Creates a validated binary snowball.
/// @return The option, or an input-validation error with a stable category.
[[nodiscard]] Result<BinarySnowballOption> make_binary_snowball_option(BinarySnowballTerms);

/// Knock-in autocallable accruing a coupon until knock-out, with downside participation.
class SnowballOption : public KnockInAutocallableNote {
public:
    /// Returns one finite knock-out coupon rate per observation date.
    const std::vector<double>& knock_out_coupon_rates() const noexcept { return knock_out_coupon_rates_; }
    /// Returns the finite maturity coupon rate.
    double maturity_coupon_rate() const noexcept { return maturity_coupon_rate_; }

    /// Compares all autocallable and coupon terms.
    friend bool operator==(const SnowballOption&, const SnowballOption&) = default;

private:
    SnowballOption(std::vector<double> knock_out_coupon_rates, double maturity_coupon_rate,
                   double initial_spot, double knock_in_level, std::vector<double> knock_out_levels,
                   double upper_strike, double lower_strike, std::vector<Date> observation_dates,
                   KnockInObservationMode knock_in_observation_mode, AutocallableBarrierState barrier_state,
                   double principal_ratio, Date effective_date, Date expiry_date)
        : KnockInAutocallableNote(initial_spot, knock_in_level, std::move(knock_out_levels), upper_strike,
                                  lower_strike, std::move(observation_dates), knock_in_observation_mode, barrier_state,
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
    /// Returns one finite knock-out coupon rate per observation date.
    const std::vector<double>& knock_out_coupon_rates() const noexcept { return knock_out_coupon_rates_; }
    /// Returns the finite flat maturity coupon rate.
    double maturity_coupon_rate() const noexcept { return maturity_coupon_rate_; }
    /// Compares all autocallable and coupon terms.
    friend bool operator==(const BinarySnowballOption&, const BinarySnowballOption&) = default;

private:
    BinarySnowballOption(std::vector<double> knock_out_coupon_rates, double maturity_coupon_rate,
                         double initial_spot, std::vector<double> knock_out_levels,
                         double upper_strike, double lower_strike, std::vector<Date> observation_dates,
                         AutocallableBarrierState barrier_state, double principal_ratio, Date effective_date,
                         Date expiry_date)
        : AutocallableNote(initial_spot, std::move(knock_out_levels), upper_strike, lower_strike,
                           std::move(observation_dates), principal_ratio, barrier_state, effective_date, expiry_date),
          knock_out_coupon_rates_(std::move(knock_out_coupon_rates)),
          maturity_coupon_rate_(maturity_coupon_rate) {}

    std::vector<double> knock_out_coupon_rates_;
    double maturity_coupon_rate_;

    friend Result<BinarySnowballOption> make_binary_snowball_option(BinarySnowballTerms);
};

/// Snowball variant whose maturity coupon steps down to a floor once knocked in.
class TernarySnowballOption : public KnockInAutocallableNote {
public:
    /// Returns one finite knock-out coupon rate per observation date.
    const std::vector<double>& knock_out_coupon_rates() const noexcept { return knock_out_coupon_rates_; }
    /// Returns the finite pre-adjustment maturity coupon rate.
    double maturity_coupon_rate() const noexcept { return maturity_coupon_rate_; }
    /// Returns the finite maturity-coupon floor after knock-in.
    double minimum_coupon_rate() const noexcept { return minimum_coupon_rate_; }
    /// Compares all autocallable and coupon terms.
    friend bool operator==(const TernarySnowballOption&, const TernarySnowballOption&) = default;

private:
    TernarySnowballOption(std::vector<double> knock_out_coupon_rates, double maturity_coupon_rate,
                          double minimum_coupon_rate, double initial_spot, double knock_in_level,
                          std::vector<double> knock_out_levels, double upper_strike,
                          double lower_strike, std::vector<Date> observation_dates,
                          KnockInObservationMode knock_in_observation_mode, AutocallableBarrierState barrier_state,
                          double principal_ratio, Date effective_date, Date expiry_date)
        : KnockInAutocallableNote(initial_spot, knock_in_level, std::move(knock_out_levels), upper_strike,
                                  lower_strike, std::move(observation_dates), knock_in_observation_mode, barrier_state,
                                  principal_ratio, effective_date, expiry_date),
          knock_out_coupon_rates_(std::move(knock_out_coupon_rates)),
          maturity_coupon_rate_(maturity_coupon_rate), minimum_coupon_rate_(minimum_coupon_rate) {}

    std::vector<double> knock_out_coupon_rates_;
    double maturity_coupon_rate_;
    double minimum_coupon_rate_;

    friend Result<TernarySnowballOption> make_ternary_snowball_option(TernarySnowballTerms);
};

[[nodiscard]] inline Result<BinarySnowballOption> make_binary_snowball_option(BinarySnowballTerms terms)
{
    return detail::validate_and_return_autocallable_note(BinarySnowballOption{std::move(terms.knock_out_coupon_rates), terms.maturity_coupon_rate,
                                                                              terms.initial_spot, std::move(terms.knock_out_levels), terms.upper_strike,
                                                                              terms.lower_strike, std::move(terms.observation_dates), terms.barrier_state,
                                                                              terms.principal_ratio, terms.effective_date, terms.expiry_date});
}

[[nodiscard]] inline Result<SnowballOption> make_snowball_option(SnowballTerms terms)
{
    return detail::validate_and_return_autocallable_note(SnowballOption{std::move(terms.knock_out_coupon_rates), terms.maturity_coupon_rate,
                                                                        terms.initial_spot, terms.knock_in_level,
                                                                        std::move(terms.knock_out_levels), terms.upper_strike, terms.lower_strike,
                                                                        std::move(terms.observation_dates), terms.knock_in_observation_mode, terms.barrier_state,
                                                                        terms.principal_ratio, terms.effective_date, terms.expiry_date});
}

[[nodiscard]] inline Result<TernarySnowballOption> make_ternary_snowball_option(TernarySnowballTerms terms)
{
    return detail::validate_and_return_autocallable_note(TernarySnowballOption{std::move(terms.knock_out_coupon_rates), terms.maturity_coupon_rate,
                                                                               terms.minimum_coupon_rate, terms.initial_spot, terms.knock_in_level,
                                                                               std::move(terms.knock_out_levels), terms.upper_strike, terms.lower_strike,
                                                                               std::move(terms.observation_dates), terms.knock_in_observation_mode, terms.barrier_state,
                                                                               terms.principal_ratio, terms.effective_date, terms.expiry_date});
}

} // namespace kiyosi
