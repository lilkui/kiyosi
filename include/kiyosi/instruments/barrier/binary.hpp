#pragma once

#include <utility>
#include <variant>
#include <vector>

#include <kiyosi/instruments/barrier/terms.hpp>
#include <kiyosi/instruments/option_terms.hpp>
#include <kiyosi/instruments/payoff.hpp>

namespace kiyosi {

class BinaryBarrierOption;
class TouchOption;

/// Input terms shared by cash and asset binary barrier options.
struct BinaryBarrierTerms {
    OptionType option_type{};                                                      ///< Call-or-put direction.
    double strike{};                                                               ///< Positive binary strike.
    Date effective_date{};                                                         ///< First date of the contract life.
    Date expiry_date{};                                                            ///< Final date of the contract life.
    double barrier_level{};                                                        ///< Positive barrier trigger level.
    BarrierType barrier_type{};                                                    ///< Barrier direction and activation behavior.
    kiyosi::ObservationMode observation_mode{kiyosi::ObservationMode::continuous}; ///< Monitoring frequency.
    std::vector<Date> observation_dates{};                                         ///< Ordered dates for scheduled monitoring.
    std::optional<BarrierTouchState> touch_state{};                                ///< History before valuation, if supplied.
};

/// Creates a validated cash-paying binary barrier option.
/// @param terms Strike, life, and barrier terms.
/// @param payout Positive finite cash payout.
/// @return The option, or an input-validation error.
[[nodiscard]] Result<BinaryBarrierOption> make_cash_binary_barrier_option(
    BinaryBarrierTerms, double payout);
/// Creates a validated asset-paying binary barrier option.
/// @return The option, or an input-validation error.
[[nodiscard]] Result<BinaryBarrierOption> make_asset_binary_barrier_option(BinaryBarrierTerms);

namespace detail {
[[nodiscard]] Result<TouchOption> make_touch_option(
    Date, Date, double, BinaryPayoff, BarrierType, SettlementTiming,
    ObservationMode, std::vector<Date>, std::optional<BarrierTouchState>);
}

/// Strike-based binary option whose payoff also depends on a barrier_level event.
class BinaryBarrierOption {
public:
    /// Returns the call-or-put direction.
    OptionType option_type() const noexcept { return terms_.option_type(); }
    /// Returns the positive strike price.
    double strike() const noexcept { return terms_.strike(); }
    /// Returns the cash-or-asset payoff.
    const BinaryPayoff& payoff() const noexcept { return payoff_; }
    /// Returns the payoff denomination.
    PayoffType payoff_type() const noexcept { return kiyosi::payoff_type(payoff_); }

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
    Date effective_date() const noexcept { return terms_.effective_date(); }
    /// Returns the final date of the contract life.
    Date expiry_date() const noexcept { return terms_.expiry_date(); }
    /// Returns the state of observations before valuation, if supplied.
    std::optional<BarrierTouchState> touch_state() const noexcept { return barrier_.touch_state(); }

    /// Compares all option, payoff, barrier, and touch-history terms.
    friend bool operator==(const BinaryBarrierOption&, const BinaryBarrierOption&) = default;

private:
    BinaryBarrierOption(OptionTerms terms, BinaryPayoff payoff, BarrierTerms barrier_terms)
        : terms_(std::move(terms)), payoff_(std::move(payoff)), barrier_(std::move(barrier_terms)) {}

    OptionTerms terms_;
    BinaryPayoff payoff_;
    BarrierTerms barrier_;

    friend Result<BinaryBarrierOption> make_cash_binary_barrier_option(
        BinaryBarrierTerms, double);
    friend Result<BinaryBarrierOption> make_asset_binary_barrier_option(BinaryBarrierTerms);
};

namespace detail {

[[nodiscard]] inline Result<std::pair<OptionTerms, BarrierTerms>> make_binary_barrier_terms(
    BinaryBarrierTerms terms)
{
    auto option = make_option_terms(terms.option_type, terms.strike, terms.effective_date, terms.expiry_date);
    if (!option) return std::unexpected(option.error());
    auto barrier_terms = make_barrier_terms(terms.barrier_level, terms.barrier_type, terms.observation_mode,
                                            std::move(terms.observation_dates), terms.effective_date, terms.expiry_date,
                                            terms.touch_state);
    if (!barrier_terms) return std::unexpected(barrier_terms.error());
    return std::pair{std::move(*option), std::move(*barrier_terms)};
}

} // namespace detail

[[nodiscard]] inline Result<BinaryBarrierOption> make_cash_binary_barrier_option(
    BinaryBarrierTerms terms, double payout)
{
    auto payoff = detail::make_cash_or_nothing_payoff(payout);
    if (!payoff) return std::unexpected(payoff.error());
    auto validated = detail::make_binary_barrier_terms(std::move(terms));
    if (!validated) return std::unexpected(validated.error());
    return BinaryBarrierOption{std::move(validated->first), std::move(*payoff),
                               std::move(validated->second)};
}

[[nodiscard]] inline Result<BinaryBarrierOption> make_asset_binary_barrier_option(
    BinaryBarrierTerms terms)
{
    auto validated = detail::make_binary_barrier_terms(std::move(terms));
    if (!validated) return std::unexpected(validated.error());
    return BinaryBarrierOption{std::move(validated->first), AssetOrNothingPayoff{},
                               std::move(validated->second)};
}

/// Strike-free one-touch or no-touch contract paying cash or the asset.
class TouchOption {
public:
    /// Returns the cash-or-asset payoff.
    const BinaryPayoff& payoff() const noexcept { return payoff_; }
    /// Returns the payoff denomination.
    PayoffType payoff_type() const noexcept { return kiyosi::payoff_type(payoff_); }
    /// Returns when the payoff settles.
    kiyosi::SettlementTiming settlement_timing() const noexcept { return settlement_timing_; }

    /// Returns the validated barrier terms.
    const BarrierTerms& barrier_terms() const noexcept { return barrier_; }
    /// Returns the positive barrier level.
    double barrier_level() const noexcept { return barrier_.barrier_level(); }
    /// Returns whether this contract pays when the barrier is touched.
    bool is_one_touch() const noexcept { return barrier_.is_knock_in(); }
    /// Returns whether this is an upward barrier.
    bool is_up() const noexcept { return barrier_.is_up(); }
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

    /// Compares the payoff, settlement timing, barrier, and touch-history terms.
    friend bool operator==(const TouchOption&, const TouchOption&) = default;

private:
    TouchOption(BinaryPayoff payoff, kiyosi::SettlementTiming settlement_timing,
                BarrierTerms barrier_terms)
        : payoff_(std::move(payoff)), settlement_timing_(settlement_timing),
          barrier_(std::move(barrier_terms))
    {
    }

    BinaryPayoff payoff_;
    kiyosi::SettlementTiming settlement_timing_;
    BarrierTerms barrier_;

    friend Result<TouchOption> detail::make_touch_option(
        Date, Date, double, BinaryPayoff, BarrierType, kiyosi::SettlementTiming,
        kiyosi::ObservationMode, std::vector<Date>, std::optional<BarrierTouchState>);
};

namespace detail {

[[nodiscard]] inline Result<TouchOption> make_touch_option(
    Date effective_date, Date expiry_date, double barrier_level, BinaryPayoff payoff, BarrierType barrier_type,
    kiyosi::SettlementTiming settlement_timing, kiyosi::ObservationMode observation_mode,
    std::vector<Date> observation_dates, std::optional<BarrierTouchState> touch_state)
{
    if (settlement_timing != kiyosi::SettlementTiming::at_hit &&
        settlement_timing != kiyosi::SettlementTiming::at_expiry)
        return std::unexpected(Error{ErrorCategory::invalid_option, "invalid settlement timing"});
    if (settlement_timing == kiyosi::SettlementTiming::at_hit &&
        !is_knock_in_barrier(barrier_type))
        return std::unexpected(Error{ErrorCategory::invalid_option,
                                     "at-hit settlement requires a one-touch"});
    auto terms = make_barrier_terms(barrier_level, barrier_type, observation_mode,
                                    std::move(observation_dates), effective_date, expiry_date, touch_state);
    if (!terms) return std::unexpected(terms.error());
    return TouchOption{std::move(payoff), settlement_timing, std::move(*terms)};
}

[[nodiscard]] inline Result<TouchOption> make_cash_touch_option(
    Date effective_date, Date expiry_date, double barrier_level, double payout, BarrierType barrier_type,
    kiyosi::SettlementTiming settlement_timing, kiyosi::ObservationMode observation_mode,
    std::vector<Date> observation_dates, std::optional<BarrierTouchState> touch_state)
{
    auto payoff = make_cash_or_nothing_payoff(payout);
    if (!payoff) return std::unexpected(payoff.error());
    return make_touch_option(effective_date, expiry_date, barrier_level, std::move(*payoff), barrier_type,
                             settlement_timing, observation_mode, std::move(observation_dates), touch_state);
}

[[nodiscard]] inline Result<TouchOption> make_asset_touch_option(
    Date effective_date, Date expiry_date, double barrier_level, BarrierType barrier_type,
    kiyosi::SettlementTiming settlement_timing, kiyosi::ObservationMode observation_mode,
    std::vector<Date> observation_dates, std::optional<BarrierTouchState> touch_state)
{
    return make_touch_option(effective_date, expiry_date, barrier_level, AssetOrNothingPayoff{}, barrier_type,
                             settlement_timing, observation_mode, std::move(observation_dates), touch_state);
}

} // namespace detail

/// Creates an up one-touch option with a fixed cash payout.
/// @return The option, or an input-validation error.
[[nodiscard]] inline Result<TouchOption> make_cash_one_touch_up(
    Date effective_date, Date expiry_date, double barrier_level, double payout,
    kiyosi::SettlementTiming settlement_timing = kiyosi::SettlementTiming::at_expiry,
    kiyosi::ObservationMode observation_mode = kiyosi::ObservationMode::continuous,
    std::vector<Date> observation_dates = {}, std::optional<BarrierTouchState> touch_state = std::nullopt)
{
    return detail::make_cash_touch_option(effective_date, expiry_date, barrier_level, payout,
                                          BarrierType::up_and_in, settlement_timing, observation_mode,
                                          std::move(observation_dates), touch_state);
}

/// Creates a down one-touch option with a fixed cash payout.
/// @return The option, or an input-validation error.
[[nodiscard]] inline Result<TouchOption> make_cash_one_touch_down(
    Date effective_date, Date expiry_date, double barrier_level, double payout,
    kiyosi::SettlementTiming settlement_timing = kiyosi::SettlementTiming::at_expiry,
    kiyosi::ObservationMode observation_mode = kiyosi::ObservationMode::continuous,
    std::vector<Date> observation_dates = {}, std::optional<BarrierTouchState> touch_state = std::nullopt)
{
    return detail::make_cash_touch_option(effective_date, expiry_date, barrier_level, payout,
                                          BarrierType::down_and_in, settlement_timing, observation_mode,
                                          std::move(observation_dates), touch_state);
}

/// Creates an up no-touch option with a fixed cash payout at expiry.
/// @return The option, or an input-validation error.
[[nodiscard]] inline Result<TouchOption> make_cash_no_touch_up(
    Date effective_date, Date expiry_date, double barrier_level, double payout,
    kiyosi::ObservationMode observation_mode = kiyosi::ObservationMode::continuous,
    std::vector<Date> observation_dates = {}, std::optional<BarrierTouchState> touch_state = std::nullopt)
{
    return detail::make_cash_touch_option(effective_date, expiry_date, barrier_level, payout,
                                          BarrierType::up_and_out, SettlementTiming::at_expiry,
                                          observation_mode, std::move(observation_dates), touch_state);
}

/// Creates a down no-touch option with a fixed cash payout at expiry.
/// @return The option, or an input-validation error.
[[nodiscard]] inline Result<TouchOption> make_cash_no_touch_down(
    Date effective_date, Date expiry_date, double barrier_level, double payout,
    kiyosi::ObservationMode observation_mode = kiyosi::ObservationMode::continuous,
    std::vector<Date> observation_dates = {}, std::optional<BarrierTouchState> touch_state = std::nullopt)
{
    return detail::make_cash_touch_option(effective_date, expiry_date, barrier_level, payout,
                                          BarrierType::down_and_out, SettlementTiming::at_expiry,
                                          observation_mode, std::move(observation_dates), touch_state);
}

/// Creates an up one-touch option paying the underlying asset.
/// @return The option, or an input-validation error.
[[nodiscard]] inline Result<TouchOption> make_asset_one_touch_up(
    Date effective_date, Date expiry_date, double barrier_level,
    kiyosi::SettlementTiming settlement_timing = kiyosi::SettlementTiming::at_expiry,
    kiyosi::ObservationMode observation_mode = kiyosi::ObservationMode::continuous,
    std::vector<Date> observation_dates = {}, std::optional<BarrierTouchState> touch_state = std::nullopt)
{
    return detail::make_asset_touch_option(effective_date, expiry_date, barrier_level, BarrierType::up_and_in,
                                           settlement_timing, observation_mode,
                                           std::move(observation_dates), touch_state);
}

/// Creates a down one-touch option paying the underlying asset.
/// @return The option, or an input-validation error.
[[nodiscard]] inline Result<TouchOption> make_asset_one_touch_down(
    Date effective_date, Date expiry_date, double barrier_level,
    kiyosi::SettlementTiming settlement_timing = kiyosi::SettlementTiming::at_expiry,
    kiyosi::ObservationMode observation_mode = kiyosi::ObservationMode::continuous,
    std::vector<Date> observation_dates = {}, std::optional<BarrierTouchState> touch_state = std::nullopt)
{
    return detail::make_asset_touch_option(effective_date, expiry_date, barrier_level, BarrierType::down_and_in,
                                           settlement_timing, observation_mode,
                                           std::move(observation_dates), touch_state);
}

/// Creates an up no-touch option paying the underlying asset at expiry.
/// @return The option, or an input-validation error.
[[nodiscard]] inline Result<TouchOption> make_asset_no_touch_up(
    Date effective_date, Date expiry_date, double barrier_level,
    kiyosi::ObservationMode observation_mode = kiyosi::ObservationMode::continuous,
    std::vector<Date> observation_dates = {}, std::optional<BarrierTouchState> touch_state = std::nullopt)
{
    return detail::make_asset_touch_option(effective_date, expiry_date, barrier_level, BarrierType::up_and_out,
                                           SettlementTiming::at_expiry, observation_mode,
                                           std::move(observation_dates), touch_state);
}

/// Creates a down no-touch option paying the underlying asset at expiry.
/// @return The option, or an input-validation error.
[[nodiscard]] inline Result<TouchOption> make_asset_no_touch_down(
    Date effective_date, Date expiry_date, double barrier_level,
    kiyosi::ObservationMode observation_mode = kiyosi::ObservationMode::continuous,
    std::vector<Date> observation_dates = {}, std::optional<BarrierTouchState> touch_state = std::nullopt)
{
    return detail::make_asset_touch_option(effective_date, expiry_date, barrier_level, BarrierType::down_and_out,
                                           SettlementTiming::at_expiry, observation_mode,
                                           std::move(observation_dates), touch_state);
}

} // namespace kiyosi
