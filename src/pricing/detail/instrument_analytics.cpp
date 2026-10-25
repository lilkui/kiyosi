#include <kiyosi/pricing/detail/instrument_analytics.hpp>

#include <algorithm>
#include <chrono>
#include <kiyosi/instruments/accumulator.hpp>
#include <kiyosi/instruments/asian.hpp>
#include <kiyosi/instruments/range_accrual.hpp>
#include <kiyosi/instruments/barrier/binary.hpp>
#include <kiyosi/instruments/barrier/option.hpp>
#include <kiyosi/pricing/detail/accumulator_program.hpp>
#include <kiyosi/pricing/detail/autocallable_traits.hpp>
#include "barrier_settlement.hpp"

namespace kiyosi::detail {

bool at_spot_discontinuity(const RangeAccrualOption& option, const PricingContext& context, double radius)
{
    const auto time = context.valuation_time();
    if (option.annual_coupon_rate() == 0.0 || time <= start_of_day(option.effective_date()) ||
        time > start_of_day(option.expiry_date()) || time != start_of_day(context.valuation_date()) ||
        !context.calendar().is_trading_day(context.valuation_date())) return false;
    const double spot = context.spot_price();
    const auto crosses = [&](double level) { return spot - radius <= level && level <= spot + radius; };
    return crosses(option.lower_bound()) || crosses(option.upper_bound());
}

Result<bool> time_stencil_available(const RangeAccrualOption& option, const PricingContext& context,
                                    Timestamp before, Timestamp after)
{
    const auto first = std::max(date_of(before), option.effective_date() + std::chrono::days{1});
    const auto last = std::min(date_of(after), option.expiry_date());
    for (Date date = first; date <= last; date += std::chrono::days{1}) {
        const auto event = start_of_day(date);
        if (before <= event && event <= after && context.calendar().is_trading_day(date)) return false;
    }
    return true;
}

bool volatility_affects_price(const RangeAccrualOption& option, const PricingContext& context)
{
    return option.annual_coupon_rate() != 0.0 && context.valuation_time() < start_of_day(option.expiry_date());
}

bool at_spot_discontinuity(const Accumulator& option, const PricingContext& context, double radius)
{
    if (context.valuation_time() != start_of_day(context.valuation_date()) ||
        !context.calendar().is_trading_day(context.valuation_date())) return false;
    const double spot = context.spot_price();
    const auto crosses = [&](double level) { return spot - radius <= level && level <= spot + radius; };
    return crosses(option.knock_out_level()) ||
           (!accumulator_knocked_out(spot, option.knock_out_level()) && crosses(option.strike()) && option.acceleration_factor() != 1.0);
}

Result<bool> time_stencil_available(const Accumulator&, const PricingContext& context,
                                    Timestamp before, Timestamp after)
{
    for (Date date = date_of(before); date <= date_of(after); date += std::chrono::days{1}) {
        const Timestamp event = start_of_day(date);
        if (before <= event && event <= after && context.calendar().is_trading_day(date)) return false;
    }
    return true;
}

bool volatility_affects_price(const Accumulator& option, const PricingContext& context)
{
    const bool knocked_out_now = context.valuation_time() == start_of_day(context.valuation_date()) &&
                                 context.calendar().is_trading_day(context.valuation_date()) &&
                                 accumulator_knocked_out(context.spot_price(), option.knock_out_level());
    // With zero rates and no new accrual, stopping an accumulated forward preserves its expectation.
    const bool exposed_quantity = option.accumulated_quantity() != 0.0 &&
                                  (!context.model_parameters().has_flat_rates() || (*context.model_parameters().risk_free_rate()) != 0.0 ||
                                   (*context.model_parameters().dividend_yield()) != 0.0);
    return !knocked_out_now && (accumulator_accrues(option) || exposed_quantity);
}

template <AveragingConvention Averaging>
Result<bool> time_stencil_available(const AveragePriceOption<Averaging>& option, const PricingContext&,
                                    Timestamp before, Timestamp after)
{
    const Timestamp averaging_start = start_of_day(option.averaging_start_date());
    return option.realized_average() == 0.0 ? after <= averaging_start : before > averaging_start;
}

bool volatility_affects_price(const ArithmeticAveragePriceOption& option, const PricingContext& context)
{
    return arithmetic_average_adjusted_strike(option, context.valuation_time()) > 0.0;
}

template <BarrierProduct Option>
bool at_spot_discontinuity(const Option& option, const PricingContext& context, double radius)
{
    const double spot = context.spot_price();
    const auto crosses = [&](double level) { return spot - radius <= level && level <= spot + radius; };
    const bool crosses_barrier = [&] {
        if constexpr (std::same_as<Option, DoubleBarrierOption> || std::same_as<Option, DoubleTouchOption>)
            return crosses(option.lower_barrier()) || crosses(option.upper_barrier());
        else return crosses(option.barrier_level());
    }();
    return option.barrier_terms().touch_state() != BarrierTouchState::touched &&
           option.barrier_terms().is_monitored_at(context.valuation_time()) &&
           crosses_barrier;
}

template <BarrierProduct Option>
bool all_greeks_unavailable(const Option& option, const PricingContext& context)
{
    return at_spot_discontinuity(option, context);
}

template <BarrierProduct Option>
Result<bool> time_stencil_available(const Option& option, const PricingContext&, Timestamp before, Timestamp after)
{
    const auto& terms = option.barrier_terms();
    if (!terms.was_touched_before(before) || !terms.was_touched_before(after)) return false;
    if (terms.is_continuous() || terms.touch_state() == BarrierTouchState::touched) return true;
    const auto& dates = terms.observation_dates();
    const auto found = std::ranges::lower_bound(dates, before);
    return found == dates.end() || *found > after;
}

template <BarrierProduct Option>
bool volatility_affects_price(const Option& option, const PricingContext& context)
{
    const auto& terms = option.barrier_terms();
    // Pricing has already validated history before checking volatility exposure.
    const auto state = *barrier_monitoring_state(terms, context.valuation_time(), context.spot_price());
    if constexpr (std::same_as<Option, TouchOption> || std::same_as<Option, DoubleTouchOption>) {
        return !state.touched && !state.finished;
    } else {
        if ((state.touched && !terms.is_knock_in()) || (!state.touched && state.finished && terms.is_knock_in())) return false;
        const bool payoff_excluded = !terms.is_knock_in() && terminal_payoff_requires_barrier_touch(terms, option.option_type(), option.strike());
        if constexpr (std::same_as<Option, BarrierOption> || std::same_as<Option, DoubleBarrierOption>) return !payoff_excluded || option.rebate() != 0.0;
        else return !payoff_excluded;
    }
}

// Current observations can make spot derivatives undefined while rate and volatility derivatives remain meaningful.
template <AutocallableProduct Note>
bool at_spot_discontinuity(const Note& option, const PricingContext& context, double radius)
{
    if (context.valuation_time() != start_of_day(context.valuation_date()) ||
        option.barrier_state() == AutocallableBarrierState::knocked_out) return false;
    const double spot = context.spot_price();
    const auto crosses = [&](double level) { return spot - radius <= level && level <= spot + radius; };
    const auto observations = option.observations_view();
    const auto found = std::ranges::lower_bound(observations, context.valuation_date(), {}, &AutocallableObservation::date);
    if (found != observations.end() && found->date == context.valuation_date()) {
        if (crosses(found->knock_out_level)) return true;
        if (spot > found->knock_out_level) return false;
        if constexpr (std::same_as<Note, PhoenixOption>)
            if (option.coupon_rate() != 0.0 && crosses(found->coupon_barrier_level)) return true;
    }
    if constexpr (!std::same_as<Note, BinarySnowballOption>)
        // Pricing has already validated history; absent initial history means not knocked in.
        return option.barrier_state() != AutocallableBarrierState::knocked_in &&
               program_knock_in_affects_payoff(autocallable_program(option)) &&
               option.knock_in_observation_mode() == KnockInObservationMode::every_trading_day &&
               context.calendar().is_trading_day(context.valuation_date()) && crosses(option.knock_in_level());
    return false;
}

template <AutocallableProduct Note>
Result<bool> time_stencil_available(const Note& option, const PricingContext& context, Timestamp before, Timestamp after)
{
    const auto crosses_event = [&](Date date) {
        const Timestamp event = start_of_day(date);
        return before <= event && event <= after;
    };
    bool available = true;
    if (option.barrier_state() == AutocallableBarrierState::knocked_out) {
        const auto shifted = make_pricing_context(context.model_parameters(), context.spot_price(), before, context.calendar());
        if (!shifted) return std::unexpected(shifted.error());
        available = validate_autocallable_history(option, *shifted).has_value();
    } else {
        const auto observations = option.observations_view();
        const auto found = std::ranges::lower_bound(observations, before, {}, &AutocallableObservation::date);
        available = found == observations.end() || found->date > after;
    }
    if constexpr (!std::same_as<Note, BinarySnowballOption>) {
        const bool daily_events = option.knock_in_observation_mode() == KnockInObservationMode::every_trading_day &&
                                  option.barrier_state() != AutocallableBarrierState::knocked_in &&
                                  option.barrier_state() != AutocallableBarrierState::knocked_out &&
                                  program_knock_in_affects_payoff(autocallable_program(option));
        if (daily_events)
            for (Date date = date_of(before); date <= date_of(after); date += std::chrono::days{1})
                if (crosses_event(date) && context.calendar().is_trading_day(date)) return false;
    }
    return available;
}

template <AutocallableProduct Note>
bool volatility_affects_price(const Note& option, const PricingContext& context)
{
    const auto program = autocallable_program(option);
    const auto initial = autocallable_initial_state(option, context, program);
    return !initial.settlement && !autocallable_fixed_value(option, context, program, initial);
}

static Result<double> shifted_maturity_coupon(
    double maturity_coupon_rate, double shift, CouponQuoteConvention convention)
{
    switch (convention) {
    case CouponQuoteConvention::shift_maturity_coupon:
        return maturity_coupon_rate + shift;
    case CouponQuoteConvention::preserve_maturity_coupon:
        return maturity_coupon_rate;
    }
    return std::unexpected(Error{ErrorCategory::invalid_parameter,
                                 "coupon quote convention is invalid"});
}

template <AutocallableProduct Note>
    requires requires(const Note& note) { note.knock_out_coupon_rates(); }
Result<Note> replace_coupon(
    const Note& option, double coupon, CouponQuoteConvention convention)
{
    const double shift = coupon - option.observations_view().front().coupon_rate;
    const auto maturity_coupon_rate = shifted_maturity_coupon(
        option.maturity_coupon_rate(), shift, convention);
    if (!maturity_coupon_rate) return std::unexpected(maturity_coupon_rate.error());
    return replace_snowball_coupons(option, coupon, *maturity_coupon_rate);
}

Result<PhoenixOption> replace_coupon(const PhoenixOption& option, double coupon)
{
    auto replaced = option;
    replaced.terms_.coupon_rate = coupon;
    const auto valid = validate_autocallable_note(replaced);
    if (!valid) return std::unexpected(valid.error());
    return replaced;
}

template KIYOSI_EXPORT bool at_spot_discontinuity(const BarrierOption&, const PricingContext&, double);
template KIYOSI_EXPORT bool at_spot_discontinuity(const DoubleBarrierOption&, const PricingContext&, double);
template KIYOSI_EXPORT bool all_greeks_unavailable(const DoubleBarrierOption&, const PricingContext&);
template KIYOSI_EXPORT Result<bool> time_stencil_available(const DoubleBarrierOption&, const PricingContext&, Timestamp, Timestamp);
template KIYOSI_EXPORT bool volatility_affects_price(const DoubleBarrierOption&, const PricingContext&);
template KIYOSI_EXPORT bool all_greeks_unavailable(const BarrierOption&, const PricingContext&);
template KIYOSI_EXPORT Result<bool> time_stencil_available(const BarrierOption&, const PricingContext&, Timestamp, Timestamp);
template KIYOSI_EXPORT bool volatility_affects_price(const BarrierOption&, const PricingContext&);
template KIYOSI_EXPORT bool at_spot_discontinuity(const BinaryBarrierOption&, const PricingContext&, double);
template KIYOSI_EXPORT bool all_greeks_unavailable(const BinaryBarrierOption&, const PricingContext&);
template KIYOSI_EXPORT Result<bool> time_stencil_available(const BinaryBarrierOption&, const PricingContext&, Timestamp, Timestamp);
template KIYOSI_EXPORT bool volatility_affects_price(const BinaryBarrierOption&, const PricingContext&);
template KIYOSI_EXPORT bool at_spot_discontinuity(const TouchOption&, const PricingContext&, double);
template KIYOSI_EXPORT bool all_greeks_unavailable(const TouchOption&, const PricingContext&);
template KIYOSI_EXPORT Result<bool> time_stencil_available(const TouchOption&, const PricingContext&, Timestamp, Timestamp);
template KIYOSI_EXPORT bool volatility_affects_price(const TouchOption&, const PricingContext&);
template KIYOSI_EXPORT bool at_spot_discontinuity(const DoubleTouchOption&, const PricingContext&, double);
template KIYOSI_EXPORT bool all_greeks_unavailable(const DoubleTouchOption&, const PricingContext&);
template KIYOSI_EXPORT Result<bool> time_stencil_available(const DoubleTouchOption&, const PricingContext&, Timestamp, Timestamp);
template KIYOSI_EXPORT bool volatility_affects_price(const DoubleTouchOption&, const PricingContext&);
template KIYOSI_EXPORT bool at_spot_discontinuity(const PhoenixOption&, const PricingContext&, double);
template KIYOSI_EXPORT Result<bool> time_stencil_available(const PhoenixOption&, const PricingContext&, Timestamp, Timestamp);
template KIYOSI_EXPORT bool volatility_affects_price(const PhoenixOption&, const PricingContext&);
template KIYOSI_EXPORT bool at_spot_discontinuity(const SnowballOption&, const PricingContext&, double);
template KIYOSI_EXPORT Result<bool> time_stencil_available(const SnowballOption&, const PricingContext&, Timestamp, Timestamp);
template KIYOSI_EXPORT bool volatility_affects_price(const SnowballOption&, const PricingContext&);
template KIYOSI_EXPORT bool at_spot_discontinuity(const BinarySnowballOption&, const PricingContext&, double);
template KIYOSI_EXPORT Result<bool> time_stencil_available(const BinarySnowballOption&, const PricingContext&, Timestamp, Timestamp);
template KIYOSI_EXPORT bool volatility_affects_price(const BinarySnowballOption&, const PricingContext&);
template KIYOSI_EXPORT bool at_spot_discontinuity(const TernarySnowballOption&, const PricingContext&, double);
template KIYOSI_EXPORT Result<bool> time_stencil_available(const TernarySnowballOption&, const PricingContext&, Timestamp, Timestamp);
template KIYOSI_EXPORT bool volatility_affects_price(const TernarySnowballOption&, const PricingContext&);
template KIYOSI_EXPORT Result<bool> time_stencil_available(const GeometricAveragePriceOption&, const PricingContext&, Timestamp, Timestamp);
template KIYOSI_EXPORT Result<bool> time_stencil_available(const ArithmeticAveragePriceOption&, const PricingContext&, Timestamp, Timestamp);
template KIYOSI_EXPORT Result<SnowballOption> replace_coupon(const SnowballOption&, double, CouponQuoteConvention);
template KIYOSI_EXPORT Result<BinarySnowballOption> replace_coupon(const BinarySnowballOption&, double, CouponQuoteConvention);
template KIYOSI_EXPORT Result<TernarySnowballOption> replace_coupon(const TernarySnowballOption&, double, CouponQuoteConvention);

} // namespace kiyosi::detail
