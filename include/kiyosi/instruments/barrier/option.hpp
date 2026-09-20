#pragma once

#include <cmath>
#include <utility>
#include <vector>

#include <kiyosi/instruments/barrier/terms.hpp>
#include <kiyosi/instruments/option_terms.hpp>

namespace kiyosi {

class BarrierOption;

struct BarrierOptionTerms {
    OptionType option_type{};
    double strike{};
    Date effective_date{};
    Date expiry_date{};
    double barrier_level{};
    BarrierType barrier_type{};
    double rebate{};
    kiyosi::RebateTiming rebate_timing{kiyosi::RebateTiming::at_expiry};
    kiyosi::ObservationMode observation_mode{kiyosi::ObservationMode::continuous};
    std::vector<Date> observation_dates{};
};

[[nodiscard]] Result<BarrierOption> make_barrier_option(BarrierOptionTerms);

/// Knock-in or knock-out vanilla payoff with an optional rebate.
class BarrierOption {
public:
    OptionType option_type() const noexcept { return option_type_; }
    double strike() const noexcept { return strike_; }
    double rebate() const noexcept { return rebate_; }
    kiyosi::RebateTiming rebate_timing() const noexcept { return rebate_timing_; }

    const BarrierTerms& barrier_terms() const noexcept { return barrier_; }
    double barrier_level() const noexcept { return barrier_.barrier_level(); }
    BarrierType barrier_type() const noexcept { return barrier_.barrier_type(); }
    kiyosi::ObservationMode observation_mode() const noexcept { return barrier_.observation_mode(); }
    const ObservationSchedule& observation_schedule() const noexcept { return barrier_.observation_schedule(); }
    const std::vector<Date>& observation_dates() const noexcept { return barrier_.observation_dates(); }
    double mean_observation_year_fraction() const noexcept { return barrier_.mean_observation_year_fraction(); }
    Date effective_date() const noexcept { return barrier_.effective_date(); }
    Date expiry_date() const noexcept { return barrier_.expiry_date(); }

    friend bool operator==(const BarrierOption&, const BarrierOption&) = default;

private:
    BarrierOption(OptionType option_type, double strike, double rebate,
                  kiyosi::RebateTiming rebate_timing,
                  BarrierTerms barrier)
        : option_type_(option_type), strike_(strike), rebate_(rebate), rebate_timing_(rebate_timing),
          barrier_(std::move(barrier))
    {
    }

    OptionType option_type_;
    double strike_;
    double rebate_;
    kiyosi::RebateTiming rebate_timing_;
    BarrierTerms barrier_;

    friend Result<BarrierOption> make_barrier_option(BarrierOptionTerms);
};

[[nodiscard]] inline Result<BarrierOption> make_barrier_option(BarrierOptionTerms terms)
{
    auto barrier_terms = detail::make_barrier_terms(terms.barrier_level, terms.barrier_type, terms.observation_mode,
                                                    std::move(terms.observation_dates), terms.effective_date, terms.expiry_date);
    if (!barrier_terms) return std::unexpected(barrier_terms.error());
    if (terms.option_type != OptionType::call && terms.option_type != OptionType::put)
        return std::unexpected(Error{ErrorCategory::invalid_option, "option type must be call or put"});
    if (!std::isfinite(terms.strike) || terms.strike <= 0.0)
        return std::unexpected(Error{ErrorCategory::invalid_strike, "strike must be finite and positive"});
    if (!std::isfinite(terms.rebate) || terms.rebate < 0.0)
        return std::unexpected(Error{ErrorCategory::invalid_parameter,
                                     "barrier terms must be finite and non-negative"});
    if (terms.rebate_timing != RebateTiming::at_hit && terms.rebate_timing != RebateTiming::at_expiry)
        return std::unexpected(Error{ErrorCategory::invalid_option, "invalid rebate timing"});
    if (barrier_terms->is_knock_in() && terms.rebate_timing == RebateTiming::at_hit)
        return std::unexpected(Error{ErrorCategory::invalid_option,
                                     "at-hit rebates are invalid for knock-in barriers"});
    return BarrierOption{terms.option_type, terms.strike, terms.rebate, terms.rebate_timing, std::move(*barrier_terms)};
}

} // namespace kiyosi
