#pragma once

#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>
#include <kiyosi/instruments/autocallable/note.hpp>

#include "calendar_dates.hpp"

namespace kiyosi::detail {

// One request binds the immutable contract and calendar whose validation it reuses.
template <AutocallableProduct Note>
class AutocallableContract {
public:
    AutocallableContract(const Note& note, const TradingCalendar& calendar)
        : note_(note), calendar_(calendar) {}

    const Note& note() const noexcept { return note_; }

    Result<void> validate(const PricingContext& context)
    {
        const auto terms = validate_terms();
        if (!terms) return terms;
        const auto life = validate_valuation_within_instrument_life(
            context.valuation_time(), note_.effective_date(), note_.expiry_date());
        if (!life) return life;
        const auto schedule = validate_schedule();
        if (!schedule) return schedule;
        const auto history = validate_autocallable_history(note_, context);
        if (!history) return history;
        return validate_trading_expiry(calendar_, note_.expiry_date());
    }

private:
    Result<void> validate_terms()
    {
        if (terms_validated_) return {};
        const auto valid = validate_autocallable_note(note_);
        if (!valid) return std::unexpected(valid.error());
        terms_validated_ = true;
        return {};
    }

    Result<void> validate_schedule()
    {
        if (schedule_validated_) return {};
        const auto terms = validate_terms();
        if (!terms) return std::unexpected(terms.error());
        const auto valid = validate_observation_trading_days(
            note_.observations_view() | std::views::transform(&AutocallableObservation::date), calendar_);
        if (!valid) return std::unexpected(valid.error());
        schedule_validated_ = true;
        return {};
    }

    const Note& note_;
    const TradingCalendar& calendar_;
    bool terms_validated_{};
    bool schedule_validated_{};
};

} // namespace kiyosi::detail
