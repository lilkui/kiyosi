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

namespace detail {

template <AutocallableProduct Note>
[[nodiscard]] Result<Note> replace_snowball_coupons(Note, double, double);
[[nodiscard]] KIYOSI_EXPORT Result<PhoenixOption> replace_coupon(const PhoenixOption&, double);

// Adapts construction terms to the immutable note validator, without copies.
template <typename Terms>
struct AutocallableInput {
    const Terms* terms{};
    double principal_ratio() const { return terms->principal_ratio; }
    Date effective_date() const { return terms->effective_date; }
    Date expiry_date() const { return terms->expiry_date; }
    auto barrier_state() const { return terms->barrier_state; }
    std::span<const AutocallableObservation> observations_view() const { return terms->observations; }
    double initial_spot() const
        requires requires { terms->initial_spot; }
    {
        return terms->initial_spot;
    }
    double upper_strike() const
        requires requires { terms->upper_strike; }
    {
        return terms->upper_strike;
    }
    double lower_strike() const
        requires requires { terms->lower_strike; }
    {
        return terms->lower_strike;
    }
    double knock_in_level() const
        requires requires { terms->knock_in_level; }
    {
        return terms->knock_in_level;
    }
    auto knock_in_observation_mode() const
        requires requires { terms->knock_in_observation_mode; }
    {
        return terms->knock_in_observation_mode;
    }
    double coupon_rate() const
        requires requires { terms->coupon_rate; }
    {
        return terms->coupon_rate;
    }
    double maturity_coupon_rate() const
        requires requires { terms->maturity_coupon_rate; }
    {
        return terms->maturity_coupon_rate;
    }
    double minimum_coupon_rate() const
        requires requires { terms->minimum_coupon_rate; }
    {
        return terms->minimum_coupon_rate;
    }
};

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
    template <AutocallableProduct Note>
    friend Result<Note> detail::replace_snowball_coupons(Note, double, double);
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
/// @tparam Note Autocallable note exposing term accessors and read-only observation records.
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
    const auto observations = note.observations_view();
    if (observations.empty())
        return std::unexpected(Error{ErrorCategory::invalid_schedule, "autocallable schedule is invalid"});
    const auto dates = observations | std::views::transform(&AutocallableObservation::date);
    auto schedule = detail::validate_date_schedule(dates, note.effective_date(), note.expiry_date());
    if (!schedule) return std::unexpected(schedule.error());
    for (const auto& observation : observations)
        if (!std::isfinite(observation.knock_out_level) || observation.knock_out_level <= 0.0)
            return std::unexpected(Error{ErrorCategory::invalid_parameter, "knock-out levels are invalid"});
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
    constexpr bool phoenix = requires { note.coupon_rate(); };
    constexpr bool snowball = requires { note.maturity_coupon_rate(); };
    if constexpr (phoenix) {
        if (!std::isfinite(note.coupon_rate()))
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
        if (!std::isfinite(note.maturity_coupon_rate()))
            return std::unexpected(Error{ErrorCategory::invalid_parameter, "maturity coupon is invalid"});
    }
    if constexpr (requires { note.minimum_coupon_rate(); }) {
        if (!std::isfinite(note.minimum_coupon_rate()) ||
            note.minimum_coupon_rate() > note.maturity_coupon_rate())
            return std::unexpected(Error{ErrorCategory::invalid_parameter, "minimum coupon is invalid"});
    }
    if constexpr (phoenix || snowball)
        for (const auto& observation : observations)
            if ((phoenix ? observation.coupon_rate : observation.coupon_barrier_level) != 0.0)
                return std::unexpected(Error{ErrorCategory::invalid_parameter, "observation contains terms unsupported by this product"});
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

// Copy once, preserving every observation association and all non-coupon terms.
template <AutocallableProduct Note>
[[nodiscard]] inline Result<Note> replace_snowball_coupons(Note note, double coupon, double maturity_coupon_rate)
{
    const double shift = coupon - note.knock_out_coupon_rates().front();
    for (auto& observation : note.terms_.observations)
        observation.coupon_rate += shift;
    note.maturity_coupon_rate_ = maturity_coupon_rate;
    const auto valid = validate_autocallable_note(note);
    if (!valid) return std::unexpected(valid.error());
    return note;
}

template <typename Terms>
[[nodiscard]] inline Result<void> validate_autocallable_terms(const Terms& terms)
{
    return validate_autocallable_note(AutocallableInput{&terms});
}

} // namespace detail

} // namespace kiyosi
