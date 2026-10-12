#pragma once

#include <utility>
#include <vector>

#include <kiyosi/instruments/autocallable/note.hpp>

namespace kiyosi {

class PhoenixOption;

/// Input terms used to construct a PhoenixOption.
struct PhoenixTerms {
    double coupon_rate{};                                    ///< Finite annualized coupon rate per observation period (ACT/365 Fixed).
    double initial_spot{};                                   ///< Positive reference spot.
    double knock_in_level{};                                 ///< Positive downside knock-in level.
    std::vector<double> knock_out_levels;                    ///< Positive knock-out levels by observation.
    std::vector<double> coupon_barrier_levels;               ///< Non-negative coupon barriers by observation.
    double upper_strike{};                                   ///< Positive upper settlement strike.
    double lower_strike{};                                   ///< Non-negative lower settlement strike.
    std::vector<Date> observation_dates;                     ///< Strictly ordered event dates.
    KnockInObservationMode knock_in_observation_mode{};      ///< Knock-in monitoring frequency.
    std::optional<AutocallableBarrierState> barrier_state{}; ///< Prior barrier state; required after monitoring begins.
    double principal_ratio{1.0};                             ///< Non-negative principal component in normalized payoff units.
    Date effective_date{};                                   ///< First date of the note life.
    Date expiry_date{};                                      ///< Final date of the note life.
};

/// Creates a validated Phoenix autocallable.
/// @return The option, or an input-validation error with a stable category.
[[nodiscard]] Result<PhoenixOption> make_phoenix_option(PhoenixTerms);

/// Knock-in autocallable paying a conditional coupon whenever spot clears the coupon barrier.
/// Prices use normalized principal units; the annual coupon accrues over each observation period.
class PhoenixOption : public KnockInAutocallableNote {
public:
    /// Returns the reference spot used to normalize contract levels.
    double initial_spot() const noexcept { return initial_spot_; }
    /// Returns the upper settlement strike.
    double upper_strike() const noexcept { return upper_strike_; }
    /// Returns the lower settlement strike.
    double lower_strike() const noexcept { return lower_strike_; }
    /// Returns the finite annualized conditional coupon rate per observation period.
    double coupon_rate() const noexcept { return coupon_rate_; }
    /// Returns one non-negative coupon barrier per observation date.
    auto coupon_barrier_levels() const noexcept { return observations_view() | std::views::transform(&AutocallableObservation::coupon_barrier_level); }

    /// Compares all autocallable and coupon terms.
    friend bool operator==(const PhoenixOption&, const PhoenixOption&) = default;

private:
    explicit PhoenixOption(PhoenixTerms terms)
        : KnockInAutocallableNote(terms.knock_in_level, detail::autocallable_observations(terms),
                                  terms.knock_in_observation_mode, terms.barrier_state,
                                  terms.principal_ratio, terms.effective_date, terms.expiry_date),
          initial_spot_(terms.initial_spot), upper_strike_(terms.upper_strike), lower_strike_(terms.lower_strike),
          coupon_rate_(terms.coupon_rate) {}

    double initial_spot_;
    double upper_strike_;
    double lower_strike_;
    double coupon_rate_;

    friend Result<PhoenixOption> make_phoenix_option(PhoenixTerms);
};

[[nodiscard]] inline Result<PhoenixOption> make_phoenix_option(PhoenixTerms terms)
{
    const auto valid = detail::validate_autocallable_terms(terms);
    if (!valid) return std::unexpected(valid.error());
    return PhoenixOption{std::move(terms)};
}

} // namespace kiyosi
