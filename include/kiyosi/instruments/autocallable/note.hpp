#pragma once

#include <cstdint>

#include <cmath>
#include <concepts>
#include <optional>
#include <ranges>
#include <span>
#include <utility>
#include <vector>

#include <kiyosi/core/error.hpp>
#include <kiyosi/instruments/autocallable/fwd.hpp>
#include <kiyosi/core/time.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/market/schedule.hpp>

namespace kiyosi {

/// Monitoring frequency for an autocallable knock-in barrier.
enum class KnockInObservationMode : std::uint8_t {
    every_trading_day, ///< Observe on every trading day in the contract life.
    at_expiry          ///< Observe only at expiry.
};

/// Barrier events known before valuation.
enum class AutocallableBarrierState : std::uint8_t {
    none,        ///< No barrier event has occurred.
    knocked_out, ///< The note has already knocked out.
    knocked_in   ///< The downside barrier has already been breached.
};

/// One contractual observation; its date, knock-out level, and coupon terms travel together.
struct AutocallableObservation {
    Date date;
    double knock_out_level;
    double coupon_rate{};          ///< Snowball knock-out annual coupon; zero for Phoenix.
    double coupon_barrier_level{}; ///< Phoenix coupon threshold; zero for snowballs.
    friend bool operator==(const AutocallableObservation&, const AutocallableObservation&) = default;
};

/// Associates parallel inputs before note construction. Omitted coupon sequences use record defaults.
/// Validates sequence lengths; contractual values and dates are validated by the note factory.
[[nodiscard]] inline Result<std::vector<AutocallableObservation>> make_autocallable_observations(
    const std::vector<Date>& dates, const std::vector<double>& levels,
    std::optional<std::vector<double>> coupons = std::nullopt,
    std::optional<std::vector<double>> coupon_barriers = std::nullopt)
{
    if (levels.size() != dates.size() || (coupons && coupons->size() != dates.size()) ||
        (coupon_barriers && coupon_barriers->size() != dates.size()))
        return std::unexpected(Error{ErrorCategory::invalid_schedule, "autocallable observation sequence lengths differ"});
    std::vector<AutocallableObservation> observations;
    observations.reserve(dates.size());
    for (std::size_t i = 0; i < dates.size(); ++i)
        observations.push_back({dates[i], levels[i], coupons ? (*coupons)[i] : 0.0,
                                coupon_barriers ? (*coupon_barriers)[i] : 0.0});
    return observations;
}

template <AutocallableProduct Note>
[[nodiscard]] Result<void> validate_autocallable_note(const Note&);

namespace detail {

template <AutocallableProduct Note>
[[nodiscard]] Result<Note> replace_snowball_coupons(Note, double, double);
[[nodiscard]] KIYOSI_EXPORT Result<PhoenixOption> replace_coupon(const PhoenixOption&, double);
template <AutocallableTerms Terms>
[[nodiscard]] Result<AutocallableNote<Terms>> make_autocallable_note(Terms);

template <typename Terms>
[[nodiscard]] inline Result<void> validate_autocallable_terms(const Terms& terms)
{
    if (!std::isfinite(terms.principal_ratio) || terms.principal_ratio < 0.0)
        return std::unexpected(Error{ErrorCategory::invalid_parameter, "autocallable terms are invalid"});
    if constexpr (std::same_as<Terms, PhoenixTerms> || std::same_as<Terms, SnowballTerms>)
        if (!std::isfinite(terms.initial_spot) || terms.initial_spot <= 0.0 ||
            !std::isfinite(terms.upper_strike) || terms.upper_strike <= 0.0 ||
            !std::isfinite(terms.lower_strike) || terms.lower_strike < 0.0 ||
            terms.lower_strike > terms.upper_strike)
            return std::unexpected(Error{ErrorCategory::invalid_parameter, "autocallable terms are invalid"});
    auto life = validate_instrument_life(terms.effective_date, terms.expiry_date);
    if (!life) return std::unexpected(life.error());
    const auto& observations = terms.observations;
    if (observations.empty())
        return std::unexpected(Error{ErrorCategory::invalid_schedule, "autocallable schedule is invalid"});
    const auto dates = observations | std::views::transform(&AutocallableObservation::date);
    auto schedule = detail::validate_date_schedule(dates, terms.effective_date, terms.expiry_date);
    if (!schedule) return std::unexpected(schedule.error());
    for (const auto& observation : observations)
        if (!std::isfinite(observation.knock_out_level) || observation.knock_out_level <= 0.0)
            return std::unexpected(Error{ErrorCategory::invalid_parameter, "knock-out levels are invalid"});
    if (terms.barrier_state && terms.barrier_state != AutocallableBarrierState::none &&
        terms.barrier_state != AutocallableBarrierState::knocked_out &&
        terms.barrier_state != AutocallableBarrierState::knocked_in)
        return std::unexpected(Error{ErrorCategory::invalid_parameter,
                                     "autocallable touch status is invalid"});
    if constexpr (std::same_as<Terms, PhoenixTerms> || std::same_as<Terms, SnowballTerms> ||
                  std::same_as<Terms, TernarySnowballTerms>) {
        if (!std::isfinite(terms.knock_in_level) || terms.knock_in_level <= 0.0 ||
            (terms.knock_in_observation_mode != KnockInObservationMode::every_trading_day &&
             terms.knock_in_observation_mode != KnockInObservationMode::at_expiry))
            return std::unexpected(Error{ErrorCategory::invalid_parameter, "knock-in terms are invalid"});
    } else {
        if (terms.barrier_state == AutocallableBarrierState::knocked_in)
            return std::unexpected(Error{ErrorCategory::invalid_parameter,
                                         "knock-in state requires a knock-in barrier"});
    }
    constexpr bool phoenix = std::same_as<Terms, PhoenixTerms>;
    constexpr bool snowball = std::same_as<Terms, SnowballTerms> || std::same_as<Terms, BinarySnowballTerms> ||
                              std::same_as<Terms, TernarySnowballTerms>;
    if constexpr (phoenix) {
        if (!std::isfinite(terms.coupon_rate))
            return std::unexpected(Error{ErrorCategory::invalid_parameter, "Phoenix coupon is invalid"});
        for (const auto& observation : observations)
            if (!std::isfinite(observation.coupon_barrier_level) || observation.coupon_barrier_level < 0.0)
                return std::unexpected(Error{ErrorCategory::invalid_parameter,
                                             "coupon barrier levels are invalid"});
    }
    if constexpr (snowball) {
        for (const auto& observation : observations)
            if (!std::isfinite(observation.coupon_rate))
                return std::unexpected(Error{ErrorCategory::invalid_parameter, "coupon rates are invalid"});
        if (!std::isfinite(terms.maturity_coupon_rate))
            return std::unexpected(Error{ErrorCategory::invalid_parameter, "maturity coupon is invalid"});
    }
    if constexpr (std::same_as<Terms, TernarySnowballTerms>) {
        if (!std::isfinite(terms.minimum_coupon_rate) ||
            terms.minimum_coupon_rate > terms.maturity_coupon_rate)
            return std::unexpected(Error{ErrorCategory::invalid_parameter, "minimum coupon is invalid"});
    }
    if constexpr (phoenix || snowball)
        for (const auto& observation : observations)
            if ((phoenix ? observation.coupon_rate : observation.coupon_barrier_level) != 0.0)
                return std::unexpected(Error{ErrorCategory::invalid_parameter, "observation contains terms unsupported by this product"});
    return {};
}

} // namespace detail

/// Complete immutable autocallable product, owning its validated construction terms.
/// Valuation excludes cashflows settled before the valuation time. Principal, coupons and
/// downside components use normalized payoff units and are added independently.
/// Observation views borrow this note. Destruction, moving from, or assignment invalidates
/// them. Convenience getters return owning copies independent of this note.
/// Concurrent const access follows the library default; assignment must not overlap access.
template <AutocallableTerms Terms>
class AutocallableNote {
public:
    AutocallableNote(const AutocallableNote&) = default;
    AutocallableNote(AutocallableNote&&) noexcept = default;
    ~AutocallableNote() = default;
    AutocallableNote& operator=(AutocallableNote&&) noexcept = default;

    /// Copies terms without changing this note if allocation fails.
    AutocallableNote& operator=(const AutocallableNote& other)
    {
        if (this != &other) {
            // Copy before replacing dates and levels so a failed allocation preserves validity.
            auto terms = other.terms_;
            terms_ = std::move(terms);
        }
        return *this;
    }

    /// Returns an owning copy of the positive knock-out levels.
    std::vector<double> knock_out_levels() const { return observations_view() | std::views::transform(&AutocallableObservation::knock_out_level) | std::ranges::to<std::vector<double>>(); }
    /// Returns an owning copy of the strictly ordered observation dates.
    std::vector<Date> observation_dates() const { return observations_view() | std::views::transform(&AutocallableObservation::date) | std::ranges::to<std::vector<Date>>(); }
    /// Borrows complete immutable observations with the lifetime documented above.
    std::span<const AutocallableObservation> observations_view() const noexcept { return terms_.observations; }
    /// Returns the non-negative normalized principal component.
    double principal_ratio() const noexcept { return terms_.principal_ratio; }
    /// Returns the first date of the note life.
    Date effective_date() const noexcept { return terms_.effective_date; }
    /// Returns the final date of the note life.
    Date expiry_date() const noexcept { return terms_.expiry_date; }
    /// Returns the prior barrier state, or nullopt for unknown history.
    std::optional<AutocallableBarrierState> barrier_state() const noexcept { return terms_.barrier_state; }
    /// Returns the positive reference spot.
    double initial_spot() const noexcept
        requires requires(const Terms& terms) { terms.initial_spot; }
    {
        return terms_.initial_spot;
    }
    /// Returns the positive upper settlement strike.
    double upper_strike() const noexcept
        requires requires(const Terms& terms) { terms.upper_strike; }
    {
        return terms_.upper_strike;
    }
    /// Returns the non-negative lower settlement strike.
    double lower_strike() const noexcept
        requires requires(const Terms& terms) { terms.lower_strike; }
    {
        return terms_.lower_strike;
    }
    /// Returns the positive downside knock-in level.
    double knock_in_level() const noexcept
        requires requires(const Terms& terms) { terms.knock_in_level; }
    {
        return terms_.knock_in_level;
    }
    /// Returns the knock-in monitoring frequency.
    KnockInObservationMode knock_in_observation_mode() const noexcept
        requires requires(const Terms& terms) { terms.knock_in_observation_mode; }
    {
        return terms_.knock_in_observation_mode;
    }
    /// Returns the annualized conditional observation coupon.
    double coupon_rate() const noexcept
        requires requires(const Terms& terms) { terms.coupon_rate; }
    {
        return terms_.coupon_rate;
    }
    /// Returns the finite maturity coupon rate.
    double maturity_coupon_rate() const noexcept
        requires requires(const Terms& terms) { terms.maturity_coupon_rate; }
    {
        return terms_.maturity_coupon_rate;
    }
    /// Returns the finite maturity-coupon floor after knock-in.
    double minimum_coupon_rate() const noexcept
        requires requires(const Terms& terms) { terms.minimum_coupon_rate; }
    {
        return terms_.minimum_coupon_rate;
    }
    /// Returns an owning copy of the finite knock-out coupon rates.
    std::vector<double> knock_out_coupon_rates() const
        requires requires(const Terms& terms) { terms.maturity_coupon_rate; }
    {
        return observations_view() | std::views::transform(&AutocallableObservation::coupon_rate) | std::ranges::to<std::vector<double>>();
    }
    /// Returns an owning copy of the non-negative coupon barriers.
    std::vector<double> coupon_barrier_levels() const
        requires requires(const Terms& terms) { terms.coupon_rate; }
    {
        return observations_view() | std::views::transform(&AutocallableObservation::coupon_barrier_level) | std::ranges::to<std::vector<double>>();
    }

    /// Compares every contractual term and observation.
    friend bool operator==(const AutocallableNote&, const AutocallableNote&) = default;

private:
    explicit AutocallableNote(Terms terms) : terms_(std::move(terms)) {}
    Terms terms_;

    template <AutocallableTerms OtherTerms>
    friend Result<AutocallableNote<OtherTerms>> detail::make_autocallable_note(OtherTerms);
    template <AutocallableProduct Note>
    friend Result<void> validate_autocallable_note(const Note&);
    template <AutocallableProduct Note>
    friend Result<Note> detail::replace_snowball_coupons(Note, double, double);
    friend Result<PhoenixOption> detail::replace_coupon(const PhoenixOption&, double);
};

/// Revalidates all terms of a complete product, including a moved-from observation schedule.
template <AutocallableProduct Note>
[[nodiscard]] inline Result<void> validate_autocallable_note(const Note& note)
{
    return detail::validate_autocallable_terms(note.terms_);
}

/// Validates the caller's history snapshot against events before valuation.
template <typename Note>
[[nodiscard]] inline Result<void> validate_autocallable_history(
    const Note& note, const PricingContext& context)
{
    const Timestamp valuation = context.valuation_time();
    const auto observations = note.observations_view();
    const bool had_knock_out_observation = !observations.empty() &&
                                           start_of_day(observations.front().date) < valuation;
    bool had_knock_in_observation = false; // NOLINT(misc-const-correctness): mutable for notes with daily monitoring.
    if constexpr (requires { note.knock_in_observation_mode(); })
        if (note.knock_in_observation_mode() == KnockInObservationMode::every_trading_day)
            for (Date date = note.effective_date(); start_of_day(date) < valuation;
                 date += std::chrono::days{1})
                if (context.calendar().is_trading_day(date)) {
                    had_knock_in_observation = true;
                    break;
                }
    if (!note.barrier_state() && (had_knock_out_observation || had_knock_in_observation))
        return std::unexpected(Error{ErrorCategory::invalid_parameter,
                                     "prior autocallable barrier state is required at valuation"});
    if (note.barrier_state() == AutocallableBarrierState::knocked_out && !had_knock_out_observation)
        return std::unexpected(Error{ErrorCategory::invalid_option,
                                     "autocallable cannot have knocked out before its first observation"});
    if (note.barrier_state() == AutocallableBarrierState::knocked_in && !had_knock_in_observation)
        return std::unexpected(Error{ErrorCategory::invalid_option,
                                     "autocallable cannot have knocked in before monitoring began"});
    return {};
}

namespace detail {

// Copy once, preserving every observation association and all non-coupon terms.
template <AutocallableProduct Note>
[[nodiscard]] inline Result<Note> replace_snowball_coupons(Note note, double coupon, double maturity_coupon_rate)
{
    const double shift = coupon - note.observations_view().front().coupon_rate;
    for (auto& observation : note.terms_.observations)
        observation.coupon_rate += shift;
    note.terms_.maturity_coupon_rate = maturity_coupon_rate;
    const auto valid = validate_autocallable_note(note);
    if (!valid) return std::unexpected(valid.error());
    return note;
}

template <AutocallableTerms Terms>
[[nodiscard]] inline Result<AutocallableNote<Terms>> make_autocallable_note(Terms terms)
{
    const auto valid = validate_autocallable_terms(terms);
    if (!valid) return std::unexpected(valid.error());
    return AutocallableNote<Terms>{std::move(terms)};
}

} // namespace detail
} // namespace kiyosi
