#pragma once

#include <cstddef>
#include <functional>
#include <vector>

#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>
#include <kiyosi/instruments/autocallable/note.hpp>

#include "calendar_dates.hpp"

namespace kiyosi::detail {

// Requires a validated schedule/calendar and resolved valuation-time events.
template <AutocallableProduct Note>
std::vector<Date> autocallable_event_dates(
    const Note& note, const PricingContext& context, std::size_t next_observation, bool daily_knock_in)
{
    if (daily_knock_in) return trading_dates(context.calendar(), context.valuation_time(), note.expiry_date());
    const auto observations = note.observations_view().subspan(next_observation) |
                              std::views::transform(&AutocallableObservation::date);
    std::vector<Date> dates(observations.begin(), observations.end());
    if (dates.empty() || dates.back() != note.expiry_date()) dates.push_back(note.expiry_date());
    return dates;
}

// One request binds the immutable contract and calendar whose validation it reuses.
template <AutocallableProduct Note>
class AutocallableContract {
public:
    AutocallableContract(const Note& note, const TradingCalendar& calendar)
        : note_(note), calendar_(calendar) {}

    const Note& note() const noexcept { return note_.get(); }

    Result<void> validate(const PricingContext& context)
    {
        if (!contract_validated_) {
            const auto terms = validate_autocallable_note(note());
            if (!terms) return terms;
        }
        const auto life = validate_valuation_within_instrument_life(
            context.valuation_time(), note().effective_date(), note().expiry_date());
        if (!life) return life;
        if (!contract_validated_) {
            const auto schedule = validate_observation_trading_days(
                note().observations_view() | std::views::transform(&AutocallableObservation::date), calendar_.get());
            if (!schedule) return schedule;
            contract_validated_ = true;
        }
        const auto history = validate_autocallable_history(note(), context);
        if (!history) return history;
        return validate_trading_expiry(calendar_.get(), note().expiry_date());
    }

private:
    std::reference_wrapper<const Note> note_;
    std::reference_wrapper<const TradingCalendar> calendar_;
    bool contract_validated_{};
};

} // namespace kiyosi::detail
