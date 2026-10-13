#pragma once

#include <utility>
#include <vector>

#include <kiyosi/instruments/autocallable/note.hpp>

namespace kiyosi {

/// Input terms used to construct a PhoenixOption.
struct PhoenixTerms {
    double coupon_rate{};                                    ///< Finite annualized coupon rate per observation period (ACT/365 Fixed).
    double initial_spot{};                                   ///< Positive reference spot.
    double knock_in_level{};                                 ///< Positive downside knock-in level.
    double upper_strike{};                                   ///< Positive upper settlement strike.
    double lower_strike{};                                   ///< Non-negative lower settlement strike.
    KnockInObservationMode knock_in_observation_mode{};      ///< Knock-in monitoring frequency.
    std::optional<AutocallableBarrierState> barrier_state{}; ///< Prior barrier state; required after monitoring begins.
    double principal_ratio{1.0};                             ///< Non-negative principal component in normalized payoff units.
    Date effective_date{};                                   ///< First date of the note life.
    Date expiry_date{};                                      ///< Final date of the note life.
    /// Complete contractual observations, with each date and its terms associated.
    std::vector<AutocallableObservation> observations{};
    friend bool operator==(const PhoenixTerms&, const PhoenixTerms&) = default;
};

/// Creates a validated Phoenix product, owning its supplied terms.
/// @return The option, or an input-validation error with a stable category.
[[nodiscard]] inline Result<PhoenixOption> make_phoenix_option(PhoenixTerms terms)
{
    return detail::make_autocallable_note(std::move(terms));
}

} // namespace kiyosi
