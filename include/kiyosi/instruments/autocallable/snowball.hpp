#pragma once

#include <utility>
#include <vector>

#include <kiyosi/instruments/autocallable/note.hpp>

namespace kiyosi {

class SnowballOption;
class BinarySnowballOption;
class TernarySnowballOption;

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
};

/// Creates a validated knock-in snowball.
/// @return The option, or an input-validation error with a stable category.
[[nodiscard]] Result<SnowballOption> make_snowball_option(const SnowballTerms&);
/// Creates a validated ternary snowball.
/// @return The option, or an input-validation error with a stable category.
[[nodiscard]] Result<TernarySnowballOption> make_ternary_snowball_option(const TernarySnowballTerms&);

/// Creates a validated binary snowball.
/// @return The option, or an input-validation error with a stable category.
[[nodiscard]] Result<BinarySnowballOption> make_binary_snowball_option(const BinarySnowballTerms&);

/// Knock-in autocallable accruing a coupon until knock-out, with downside participation.
class SnowballOption : public KnockInAutocallableNote {
public:
    /// Returns the reference spot used to normalize contract levels.
    double initial_spot() const noexcept { return initial_spot_; }
    /// Returns the upper settlement strike.
    double upper_strike() const noexcept { return upper_strike_; }
    /// Returns the lower settlement strike.
    double lower_strike() const noexcept { return lower_strike_; }
    /// Returns one finite knock-out coupon rate per observation date.
    auto knock_out_coupon_rates() const noexcept { return observations_view() | std::views::transform(&AutocallableObservation::coupon_rate); }
    /// Returns the finite maturity coupon rate.
    double maturity_coupon_rate() const noexcept { return maturity_coupon_rate_; }

    /// Compares all autocallable and coupon terms.
    friend bool operator==(const SnowballOption&, const SnowballOption&) = default;

private:
    explicit SnowballOption(const SnowballTerms& terms)
        : KnockInAutocallableNote(terms.knock_in_level, terms.observations,
                                  terms.knock_in_observation_mode, terms.barrier_state,
                                  terms.principal_ratio, terms.effective_date, terms.expiry_date),
          initial_spot_(terms.initial_spot), upper_strike_(terms.upper_strike), lower_strike_(terms.lower_strike),
          maturity_coupon_rate_(terms.maturity_coupon_rate) {}

    double initial_spot_;
    double upper_strike_;
    double lower_strike_;
    double maturity_coupon_rate_;

    friend Result<SnowballOption> make_snowball_option(const SnowballTerms&);
    template <AutocallableProduct Note>
    friend Result<Note> detail::replace_snowball_coupons(Note, double, double);
};

/// Snowball variant settling a flat coupon at maturity regardless of the terminal spot.
class BinarySnowballOption : public AutocallableNote {
public:
    /// Returns one finite knock-out coupon rate per observation date.
    auto knock_out_coupon_rates() const noexcept { return observations_view() | std::views::transform(&AutocallableObservation::coupon_rate); }
    /// Returns the finite flat maturity coupon rate.
    double maturity_coupon_rate() const noexcept { return maturity_coupon_rate_; }
    /// Compares all autocallable and coupon terms.
    friend bool operator==(const BinarySnowballOption&, const BinarySnowballOption&) = default;

private:
    explicit BinarySnowballOption(const BinarySnowballTerms& terms)
        : AutocallableNote(terms.observations, terms.principal_ratio, terms.barrier_state,
                           terms.effective_date, terms.expiry_date),
          maturity_coupon_rate_(terms.maturity_coupon_rate) {}

    double maturity_coupon_rate_;

    friend Result<BinarySnowballOption> make_binary_snowball_option(const BinarySnowballTerms&);
    template <AutocallableProduct Note>
    friend Result<Note> detail::replace_snowball_coupons(Note, double, double);
};

/// Snowball variant whose maturity coupon steps down to a floor once knocked in.
class TernarySnowballOption : public KnockInAutocallableNote {
public:
    /// Returns one finite knock-out coupon rate per observation date.
    auto knock_out_coupon_rates() const noexcept { return observations_view() | std::views::transform(&AutocallableObservation::coupon_rate); }
    /// Returns the finite pre-adjustment maturity coupon rate.
    double maturity_coupon_rate() const noexcept { return maturity_coupon_rate_; }
    /// Returns the finite maturity-coupon floor after knock-in.
    double minimum_coupon_rate() const noexcept { return minimum_coupon_rate_; }
    /// Compares all autocallable and coupon terms.
    friend bool operator==(const TernarySnowballOption&, const TernarySnowballOption&) = default;

private:
    explicit TernarySnowballOption(const TernarySnowballTerms& terms)
        : KnockInAutocallableNote(terms.knock_in_level, terms.observations, terms.knock_in_observation_mode,
                                  terms.barrier_state, terms.principal_ratio, terms.effective_date, terms.expiry_date),
          maturity_coupon_rate_(terms.maturity_coupon_rate), minimum_coupon_rate_(terms.minimum_coupon_rate) {}

    double maturity_coupon_rate_;
    double minimum_coupon_rate_;

    friend Result<TernarySnowballOption> make_ternary_snowball_option(const TernarySnowballTerms&);
    template <AutocallableProduct Note>
    friend Result<Note> detail::replace_snowball_coupons(Note, double, double);
};

[[nodiscard]] inline Result<BinarySnowballOption> make_binary_snowball_option(const BinarySnowballTerms& terms)
{
    const auto valid = detail::validate_autocallable_terms(terms);
    if (!valid) return std::unexpected(valid.error());
    return BinarySnowballOption{terms};
}

[[nodiscard]] inline Result<SnowballOption> make_snowball_option(const SnowballTerms& terms)
{
    const auto valid = detail::validate_autocallable_terms(terms);
    if (!valid) return std::unexpected(valid.error());
    return SnowballOption{terms};
}

[[nodiscard]] inline Result<TernarySnowballOption> make_ternary_snowball_option(const TernarySnowballTerms& terms)
{
    const auto valid = detail::validate_autocallable_terms(terms);
    if (!valid) return std::unexpected(valid.error());
    return TernarySnowballOption{terms};
}

} // namespace kiyosi
