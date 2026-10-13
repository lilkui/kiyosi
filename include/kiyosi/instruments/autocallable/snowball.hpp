#pragma once

#include <utility>
#include <vector>

#include <kiyosi/instruments/autocallable/note.hpp>

namespace kiyosi {

/// Input terms used to construct a SnowballOption.
struct SnowballTerms {
    double maturity_coupon_rate{};                           ///< Finite coupon paid at maturity when applicable.
    double initial_spot{};                                   ///< Positive reference spot.
    double knock_in_level{};                                 ///< Positive downside knock-in level.
    double upper_strike{};                                   ///< Positive upper settlement strike.
    double lower_strike{};                                   ///< Non-negative lower settlement strike.
    KnockInObservationMode knock_in_observation_mode{};      ///< Knock-in monitoring frequency.
    std::optional<AutocallableBarrierState> barrier_state{}; ///< Prior barrier state; required after monitoring begins.
    double principal_ratio{1.0};                             ///< Normalized principal repayment component.
    Date effective_date{};                                   ///< First date of the note life.
    Date expiry_date{};                                      ///< Final date of the note life.
    /// Complete contractual observations, with each date and its terms associated.
    std::vector<AutocallableObservation> observations{};
    friend bool operator==(const SnowballTerms&, const SnowballTerms&) = default;
};

/// Input terms used to construct a TernarySnowballOption.
struct TernarySnowballTerms {
    double maturity_coupon_rate{};                           ///< Finite coupon paid at maturity before knock-in adjustment.
    double minimum_coupon_rate{};                            ///< Finite maturity-coupon floor after knock-in.
    double knock_in_level{};                                 ///< Positive downside knock-in level.
    KnockInObservationMode knock_in_observation_mode{};      ///< Knock-in monitoring frequency.
    std::optional<AutocallableBarrierState> barrier_state{}; ///< Prior barrier state; required after monitoring begins.
    double principal_ratio{1.0};                             ///< Normalized principal repayment component.
    Date effective_date{};                                   ///< First date of the note life.
    Date expiry_date{};                                      ///< Final date of the note life.
    /// Complete contractual observations, with each date and its terms associated.
    std::vector<AutocallableObservation> observations{};
    friend bool operator==(const TernarySnowballTerms&, const TernarySnowballTerms&) = default;
};

/// Input terms used to construct a BinarySnowballOption.
struct BinarySnowballTerms {
    double maturity_coupon_rate{};                           ///< Finite flat maturity coupon.
    std::optional<AutocallableBarrierState> barrier_state{}; ///< Prior barrier state; required after monitoring begins.
    double principal_ratio{1.0};                             ///< Normalized principal repayment component.
    Date effective_date{};                                   ///< First date of the note life.
    Date expiry_date{};                                      ///< Final date of the note life.
    /// Complete contractual observations, with each date and its terms associated.
    std::vector<AutocallableObservation> observations{};
    friend bool operator==(const BinarySnowballTerms&, const BinarySnowballTerms&) = default;
};

/// Creates a validated Snowball product, owning its supplied terms.
/// @return The option, or an input-validation error with a stable category.
[[nodiscard]] inline Result<SnowballOption> make_snowball_option(SnowballTerms terms)
{
    return detail::make_autocallable_note(std::move(terms));
}

/// Creates a validated TernarySnowball product, owning its supplied terms.
/// @return The option, or an input-validation error with a stable category.
[[nodiscard]] inline Result<TernarySnowballOption> make_ternary_snowball_option(TernarySnowballTerms terms)
{
    return detail::make_autocallable_note(std::move(terms));
}

/// Creates a validated BinarySnowball product, owning its supplied terms.
/// @return The option, or an input-validation error with a stable category.
[[nodiscard]] inline Result<BinarySnowballOption> make_binary_snowball_option(BinarySnowballTerms terms)
{
    return detail::make_autocallable_note(std::move(terms));
}

} // namespace kiyosi
