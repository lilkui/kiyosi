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
#include <kiyosi/core/time.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/market/schedule.hpp>

namespace kiyosi {

class PhoenixOption;
class SnowballOption;
class BinarySnowballOption;
class TernarySnowballOption;

/// Complete products supported by the autocallable pricing engines.
template <typename Note>
concept AutocallableProduct = std::same_as<Note, PhoenixOption> ||
                              std::same_as<Note, SnowballOption> ||
                              std::same_as<Note, BinarySnowballOption> ||
                              std::same_as<Note, TernarySnowballOption>;

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

namespace detail {

// Adapts sequence inputs to the same validator used by immutable notes, without copies.
template <typename Terms>
struct AutocallableInput {
    const Terms& terms;
    double principal_ratio() const { return terms.principal_ratio; }
    Date effective_date() const { return terms.effective_date; }
    Date expiry_date() const { return terms.expiry_date; }
    auto barrier_state() const { return terms.barrier_state; }
    const auto& observation_dates() const { return terms.observation_dates; }
    const auto& knock_out_levels() const { return terms.knock_out_levels; }
    double initial_spot() const
        requires requires { terms.initial_spot; }
    {
        return terms.initial_spot;
    }
    double upper_strike() const
        requires requires { terms.upper_strike; }
    {
        return terms.upper_strike;
    }
    double lower_strike() const
        requires requires { terms.lower_strike; }
    {
        return terms.lower_strike;
    }
    double knock_in_level() const
        requires requires { terms.knock_in_level; }
    {
        return terms.knock_in_level;
    }
    auto knock_in_observation_mode() const
        requires requires { terms.knock_in_observation_mode; }
    {
        return terms.knock_in_observation_mode;
    }
    double coupon_rate() const
        requires requires { terms.coupon_rate; }
    {
        return terms.coupon_rate;
    }
    const auto& coupon_barrier_levels() const
        requires requires { terms.coupon_barrier_levels; }
    {
        return terms.coupon_barrier_levels;
    }
    const auto& knock_out_coupon_rates() const
        requires requires { terms.knock_out_coupon_rates; }
    {
        return terms.knock_out_coupon_rates;
    }
    double maturity_coupon_rate() const
        requires requires { terms.maturity_coupon_rate; }
    {
        return terms.maturity_coupon_rate;
    }
    double minimum_coupon_rate() const
        requires requires { terms.minimum_coupon_rate; }
    {
        return terms.minimum_coupon_rate;
    }
};

// Input validation establishes equal sequence lengths before records are constructed.
template <typename Terms>
std::vector<AutocallableObservation> autocallable_observations(const Terms& terms)
{
    std::vector<AutocallableObservation> observations;
    observations.reserve(terms.observation_dates.size());
    for (std::size_t i = 0; i < terms.observation_dates.size(); ++i) {
        AutocallableObservation observation{terms.observation_dates[i], terms.knock_out_levels[i]};
        if constexpr (requires { terms.knock_out_coupon_rates; }) observation.coupon_rate = terms.knock_out_coupon_rates[i];
        if constexpr (requires { terms.coupon_barrier_levels; }) observation.coupon_barrier_level = terms.coupon_barrier_levels[i];
        observations.push_back(observation);
    }
    return observations;
}

} // namespace detail

/// Principal, knock-out ladder, and contract life shared by every autocallable structure.
/// Valuation excludes cashflows settled before the valuation time.
/// For example, principal_ratio 2 plus a 0.1 coupon pays 2.1 normalized units;
/// principal_ratio 2 plus a -0.2 downside component pays 1.8.
/// Observation views are read-only and borrow this note. Destruction, moving from, or assignment
/// invalidates them; projection iterators also require their view object to remain alive.
/// Concurrent const access follows the library default; assignment must not overlap any access.
class AutocallableNote {
public:
    AutocallableNote(const AutocallableNote&) = default;
    AutocallableNote(AutocallableNote&&) = default;
    ~AutocallableNote() = default;

    /// Returns one positive knock-out level per observation date.
    auto knock_out_levels() const noexcept { return observations_view() | std::views::transform(&AutocallableObservation::knock_out_level); }
    /// Returns the strictly ordered knock-out observation dates.
    auto observation_dates() const noexcept { return observations_view() | std::views::transform(&AutocallableObservation::date); }
    /// Borrows the complete immutable observation records, with the lifetime documented above.
    std::span<const AutocallableObservation> observations_view() const noexcept { return terms_.observations; }
    /// Returns the non-negative normalized principal repayment component.
    double principal_ratio() const noexcept { return terms_.principal_ratio; }
    /// Returns the first date of the note life.
    Date effective_date() const noexcept { return terms_.effective_date; }
    /// Returns the final date of the note life.
    Date expiry_date() const noexcept { return terms_.expiry_date; }
    /// Returns the supplied state before valuation, or nullopt when history is unknown.
    std::optional<AutocallableBarrierState> barrier_state() const noexcept { return terms_.barrier_state; }
    /// Compares all shared autocallable terms.
    friend bool operator==(const AutocallableNote&, const AutocallableNote&) = default;

protected:
    // Only complete products may replace shared terms and their observations.
    AutocallableNote& operator=(const AutocallableNote&) = default;
    AutocallableNote& operator=(AutocallableNote&&) = default;

private:
    AutocallableNote(std::vector<AutocallableObservation> observations, double principal_ratio,
                     std::optional<AutocallableBarrierState> barrier_state, Date effective_date, Date expiry_date)
        : terms_{std::move(observations), principal_ratio,
                 barrier_state, effective_date, expiry_date} {}

    struct Terms {
        std::vector<AutocallableObservation> observations;
        double principal_ratio;
        std::optional<AutocallableBarrierState> barrier_state;
        Date effective_date;
        Date expiry_date;
        friend bool operator==(const Terms&, const Terms&) = default;
    } terms_;

    friend class KnockInAutocallableNote;
    friend class BinarySnowballOption;
};

/// An autocallable note carrying a downside knock-in barrier.
class KnockInAutocallableNote : public AutocallableNote {
public:
    KnockInAutocallableNote(const KnockInAutocallableNote&) = default;
    KnockInAutocallableNote(KnockInAutocallableNote&&) = default;
    ~KnockInAutocallableNote() = default;

    /// Returns the positive downside knock-in level.
    double knock_in_level() const noexcept { return knock_in_level_; }
    /// Returns the knock-in monitoring frequency.
    KnockInObservationMode knock_in_observation_mode() const noexcept { return knock_in_observation_mode_; }
    /// Compares all shared terms and knock-in terms.
    friend bool operator==(const KnockInAutocallableNote&, const KnockInAutocallableNote&) = default;

protected:
    // Only complete products may replace shared terms and their observations.
    KnockInAutocallableNote& operator=(const KnockInAutocallableNote&) = default;
    KnockInAutocallableNote& operator=(KnockInAutocallableNote&&) = default;

private:
    KnockInAutocallableNote(double knock_in_level, std::vector<AutocallableObservation> observations,
                            KnockInObservationMode knock_in_observation_mode, std::optional<AutocallableBarrierState> barrier_state,
                            double principal_ratio, Date effective_date, Date expiry_date)
        : AutocallableNote(std::move(observations),
                           principal_ratio, barrier_state, effective_date, expiry_date),
          knock_in_level_(knock_in_level), knock_in_observation_mode_(knock_in_observation_mode) {}

    double knock_in_level_;
    KnockInObservationMode knock_in_observation_mode_;

    friend class PhoenixOption;
    friend class SnowballOption;
    friend class TernarySnowballOption;
};

/// Authoritative domain validation for every autocallable product; optional features are
/// detected structurally so each product only pays for the checks it needs.
/// Pass the concrete product for complete validation; base references validate only shared terms.
/// @tparam Note Autocallable note exposing the required term accessors.
/// @return Success, or an input-validation error with a stable category.
template <typename Note>
[[nodiscard]] inline Result<void> validate_autocallable_note(const Note& note)
{
    if (!std::isfinite(note.principal_ratio()) || note.principal_ratio() < 0.0)
        return std::unexpected(Error{ErrorCategory::invalid_parameter, "autocallable terms are invalid"});
    if constexpr (requires { note.initial_spot(); note.upper_strike(); note.lower_strike(); })
        if (!std::isfinite(note.initial_spot()) || note.initial_spot() <= 0.0 ||
            !std::isfinite(note.upper_strike()) || note.upper_strike() <= 0.0 ||
            !std::isfinite(note.lower_strike()) || note.lower_strike() < 0.0 ||
            note.lower_strike() > note.upper_strike())
            return std::unexpected(Error{ErrorCategory::invalid_parameter, "autocallable terms are invalid"});
    auto life = validate_instrument_life(note.effective_date(), note.expiry_date());
    if (!life) return std::unexpected(life.error());
    if (note.observation_dates().empty() ||
        note.knock_out_levels().size() != note.observation_dates().size())
        return std::unexpected(Error{ErrorCategory::invalid_schedule, "autocallable schedule is invalid"});
    auto schedule = detail::validate_date_schedule(note.observation_dates(), note.effective_date(), note.expiry_date());
    if (!schedule) return std::unexpected(schedule.error());
    for (std::size_t index = 0; index < note.observation_dates().size(); ++index) {
        if (!std::isfinite(note.knock_out_levels()[index]) || note.knock_out_levels()[index] <= 0.0)
            return std::unexpected(Error{ErrorCategory::invalid_parameter, "knock-out levels are invalid"});
    }
    if (note.barrier_state() && note.barrier_state() != AutocallableBarrierState::none &&
        note.barrier_state() != AutocallableBarrierState::knocked_out &&
        note.barrier_state() != AutocallableBarrierState::knocked_in)
        return std::unexpected(Error{ErrorCategory::invalid_parameter,
                                     "autocallable touch status is invalid"});
    if constexpr (requires { note.knock_in_level(); }) {
        if (!std::isfinite(note.knock_in_level()) || note.knock_in_level() <= 0.0 ||
            (note.knock_in_observation_mode() != KnockInObservationMode::every_trading_day &&
             note.knock_in_observation_mode() != KnockInObservationMode::at_expiry))
            return std::unexpected(Error{ErrorCategory::invalid_parameter, "knock-in terms are invalid"});
    } else if constexpr (!std::same_as<Note, AutocallableNote>) {
        if (note.barrier_state() == AutocallableBarrierState::knocked_in)
            return std::unexpected(Error{ErrorCategory::invalid_parameter,
                                         "knock-in state requires a knock-in barrier"});
    }
    if constexpr (requires { note.coupon_rate(); }) {
        if (!std::isfinite(note.coupon_rate()))
            return std::unexpected(Error{ErrorCategory::invalid_parameter, "Phoenix coupon is invalid"});
        if (note.coupon_barrier_levels().size() != note.observation_dates().size())
            return std::unexpected(Error{ErrorCategory::invalid_schedule,
                                         "Phoenix coupon schedule is invalid"});
        for (const double barrier : note.coupon_barrier_levels())
            if (!std::isfinite(barrier) || barrier < 0.0)
                return std::unexpected(Error{ErrorCategory::invalid_parameter,
                                             "coupon barrier levels are invalid"});
    }
    if constexpr (requires { note.knock_out_coupon_rates(); }) {
        if (note.knock_out_coupon_rates().size() != note.observation_dates().size())
            return std::unexpected(Error{ErrorCategory::invalid_schedule,
                                         "coupon schedule count is invalid"});
        for (const double coupon : note.knock_out_coupon_rates())
            if (!std::isfinite(coupon))
                return std::unexpected(Error{ErrorCategory::invalid_parameter, "coupon rates are invalid"});
        if (!std::isfinite(note.maturity_coupon_rate()))
            return std::unexpected(Error{ErrorCategory::invalid_parameter, "maturity coupon is invalid"});
    }
    if constexpr (requires { note.minimum_coupon_rate(); }) {
        if (!std::isfinite(note.minimum_coupon_rate()) ||
            note.minimum_coupon_rate() > note.maturity_coupon_rate())
            return std::unexpected(Error{ErrorCategory::invalid_parameter, "minimum coupon is invalid"});
    }
    return {};
}

/// Validates the caller's history snapshot against events before valuation.
template <typename Note>
[[nodiscard]] inline Result<void> validate_autocallable_history(
    const Note& note, const PricingContext& context)
{
    const Timestamp valuation = context.valuation_time();
    const bool had_knock_out_observation = !note.observation_dates().empty() &&
                                           start_of_day(note.observation_dates().front()) < valuation;
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

template <typename Terms>
[[nodiscard]] inline Result<void> validate_autocallable_terms(const Terms& terms)
{
    return validate_autocallable_note(AutocallableInput{terms});
}

} // namespace detail

} // namespace kiyosi
