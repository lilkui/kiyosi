#pragma once

#include <cmath>
#include <utility>
#include <vector>

#include <kiyosi/instruments/barrier/terms.hpp>
#include <kiyosi/instruments/option_terms.hpp>

namespace kiyosi {

class BarrierOption;

/// Input terms used to construct a BarrierOption.
struct BarrierOptionTerms {
    OptionType option_type{};                                                      ///< Call-or-put direction.
    double strike{};                                                               ///< Positive option strike.
    Date effective_date{};                                                         ///< First date of the contract life.
    Date expiry_date{};                                                            ///< Final date of the contract life.
    double barrier_level{};                                                        ///< Positive barrier trigger level.
    BarrierType barrier_type{};                                                    ///< Barrier direction and activation behavior.
    double rebate{};                                                               ///< Non-negative cash rebate.
    kiyosi::RebateTiming rebate_timing{kiyosi::RebateTiming::at_expiry};           ///< Rebate payment timing.
    kiyosi::ObservationMode observation_mode{kiyosi::ObservationMode::continuous}; ///< Monitoring frequency.
    std::vector<Date> observation_dates{};                                         ///< Ordered dates for scheduled monitoring.
    std::optional<BarrierTouchState> touch_state{};                                ///< History before valuation, if supplied.
};

/// Creates a validated barrier option.
/// @return The option, or an input-validation error.
[[nodiscard]] Result<BarrierOption> make_barrier_option(BarrierOptionTerms);

/// Knock-in or knock-out vanilla payoff with an optional rebate.
class BarrierOption {
public:
    /// Returns the call-or-put direction.
    OptionType option_type() const noexcept { return option_type_; }
    /// Returns the positive strike price.
    double strike() const noexcept { return strike_; }
    /// Returns the non-negative rebate.
    double rebate() const noexcept { return rebate_; }
    /// Returns when the rebate is paid.
    kiyosi::RebateTiming rebate_timing() const noexcept { return rebate_timing_; }

    /// Returns the validated barrier terms.
    const BarrierTerms& barrier_terms() const noexcept { return barrier_; }
    /// Returns the positive barrier level.
    double barrier_level() const noexcept { return barrier_.barrier_level(); }
    /// Returns the barrier direction and activation behavior.
    BarrierType barrier_type() const noexcept { return barrier_.barrier_type(); }
    /// Returns the monitoring frequency.
    kiyosi::ObservationMode observation_mode() const noexcept { return barrier_.observation_mode(); }
    /// Returns the validated observation schedule.
    const ObservationSchedule& observation_schedule() const noexcept { return barrier_.observation_schedule(); }
    /// Returns the ordered observation dates.
    const std::vector<Date>& observation_dates() const noexcept { return barrier_.observation_dates(); }
    /// Returns the average spacing between scheduled observations in years.
    double mean_observation_year_fraction() const noexcept { return barrier_.mean_observation_year_fraction(); }
    /// Returns the first date of the contract life.
    Date effective_date() const noexcept { return barrier_.effective_date(); }
    /// Returns the final date of the contract life.
    Date expiry_date() const noexcept { return barrier_.expiry_date(); }
    /// Returns the state of observations before valuation, if supplied.
    std::optional<BarrierTouchState> touch_state() const noexcept { return barrier_.touch_state(); }

    /// Compares all option, rebate, barrier, and touch-history terms.
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
                                                    std::move(terms.observation_dates), terms.effective_date, terms.expiry_date,
                                                    terms.touch_state);
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
