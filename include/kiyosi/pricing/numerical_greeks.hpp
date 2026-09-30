#pragma once

#include <algorithm>
#include <chrono>
#include <cmath>
#include <optional>
#include <ratio>
#include <random>

#include <kiyosi/instruments/barrier/terms.hpp>
#include <kiyosi/instruments/structured/autocallable.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>
#include <kiyosi/pricing/settings/numerical_shift.hpp>

namespace kiyosi {

namespace detail {

template <typename Engine, typename Option>
[[nodiscard]] Result<double> numerical_value(
    const Engine& engine, const Option& option, const PricingContext& context)
{
    auto priced = engine.price(option, context);
    if (!priced) return std::unexpected(priced.error());
    if (!std::isfinite(*priced))
        return std::unexpected(Error{ErrorCategory::invalid_result, "pricing produced no finite price"});
    return *priced;
}

[[nodiscard]] inline Result<PricingContext> shifted_context(
    const PricingContext& context, double spot, double volatility, double rate, Timestamp valuation)
{
    auto parameters = make_bsm_parameters(rate, context.model_parameters().dividend_yield(), volatility);
    if (!parameters) return std::unexpected(parameters.error());
    return make_pricing_context(*parameters, spot, valuation, context.calendar());
}

template <typename Engine, typename Option>
[[nodiscard]] Result<double> shifted_value(
    const Engine& engine, const Option& option, const PricingContext& context, double spot,
    double volatility, double rate, Timestamp valuation)
{
    auto shifted = shifted_context(context, spot, volatility, rate, valuation);
    if (!shifted) return std::unexpected(shifted.error());
    return numerical_value(engine, option, *shifted);
}

/// Extracts a checked scalar from the private native engine result.
inline Result<double> price_value(const Result<PricingResult>& result)
{
    if (!result) return std::unexpected(result.error());
    const auto value = result->price();
    if (!std::isfinite(value))
        return std::unexpected(Error{ErrorCategory::invalid_result, "pricing produced no finite price"});
    return value;
}

inline Result<void> validate_greeks_settings(NumericalShiftSettings settings)
{
    if (!std::isfinite(settings.spot_shift) || settings.spot_shift <= 0.0 ||
        !std::isfinite(settings.volatility_shift) || settings.volatility_shift <= 0.0 ||
        !std::isfinite(settings.rate_shift) || settings.rate_shift <= 0.0 ||
        settings.time_shift_days <= 0)
        return std::unexpected(Error{ErrorCategory::invalid_parameter, "numerical shifts are invalid"});
    return {};
}

template <typename Option>
bool greeks_unavailable(const Option& option, const PricingContext& context)
{
    if constexpr (requires { option.expiry_date(); })
        if (context.valuation_time() == start_of_day(option.expiry_date())) return true;
    if constexpr (requires { option.barrier_terms(); })
        if (option.barrier_terms().touch_state() != BarrierTouchState::touched &&
            option.barrier_terms().is_monitored_at(context.valuation_time()) &&
            context.spot_price() == option.barrier_level()) return true;
    return false;
}

// Current observation events can make spot derivatives undefined while rate and
// volatility derivatives (with the event branch held fixed) remain meaningful.
template <typename Option>
bool at_spot_discontinuity(const Option& option, const PricingContext& context)
{
    if (context.valuation_time() != start_of_day(context.valuation_date())) return false;
    const double spot = context.spot_price();
    if constexpr (requires { option.accumulated_quantity(); }) {
        if (context.calendar().is_trading_day(context.valuation_date()))
            return spot == option.knock_out_level() ||
                   (spot < option.knock_out_level() && spot == option.strike() &&
                    option.acceleration_factor() != 1.0);
    }
    if constexpr (requires { option.knock_out_levels(); option.barrier_state(); }) {
        if (option.barrier_state() == AutocallableBarrierState::knocked_out) return false;
        for (std::size_t index = 0; index < option.observation_dates().size(); ++index) {
            if (option.observation_dates()[index] != context.valuation_date()) continue;
            if (spot == option.knock_out_levels()[index]) return true;
            if constexpr (requires { option.coupon_barrier_levels(); })
                if (option.coupon_rate() != 0.0 && spot == option.coupon_barrier_levels()[index])
                    return true;
            if (spot > option.knock_out_levels()[index]) return false;
        }
        if constexpr (requires { option.knock_in_level(); })
            // Native pricing has already validated history; absent initial history
            // therefore means not knocked in, just as in the pricing engines.
            return option.barrier_state() != AutocallableBarrierState::knocked_in &&
                   option.knock_in_observation_mode() == KnockInObservationMode::every_trading_day &&
                   context.calendar().is_trading_day(context.valuation_date()) &&
                   spot == option.knock_in_level();
    }
    return false;
}

// Fills only missing requested measures. A supplied native value is never overwritten.
template <typename Engine, typename Option>
Result<PricingResult> complete_greeks(
    const Engine& engine, const Option& option, const PricingContext& context,
    GreeksRequest greeks, NumericalShiftSettings settings, const PricingResult& native)
{
    const double spot = context.spot_price();
    const double volatility = context.model_parameters().volatility();
    const double rate = context.model_parameters().risk_free_rate();
    const Timestamp valuation_time = context.valuation_time();
    const Result<double> p0 = native.price();
    if (greeks_unavailable(option, context))
        return make_pricing_result(*p0);
    const auto need = [&](Greek measure) {
        return greeks.has(measure) && !native.has(measure);
    };
    const double h = settings.spot_shift;
    const bool spot_discontinuity = at_spot_discontinuity(option, context);
    const bool spot_stencil_available = !spot_discontinuity && spot > h &&
                                        std::isfinite(spot + h) && spot + h > spot && spot - h < spot;
    auto delta = *native.get(Greek::delta);
    auto gamma = *native.get(Greek::gamma);
    auto speed = *native.get(Greek::speed);
    if (spot_stencil_available && (need(Greek::delta) || need(Greek::gamma) || need(Greek::speed))) {
        const auto p_up = detail::shifted_value(
            engine, option, context, spot + h, volatility, rate, valuation_time);
        if (!p_up) return std::unexpected(p_up.error());
        const auto p_down = detail::shifted_value(
            engine, option, context, spot - h, volatility, rate, valuation_time);
        if (!p_down) return std::unexpected(p_down.error());
        if (need(Greek::delta)) delta = (*p_up - *p_down) / (2.0 * h);
        if (need(Greek::gamma)) gamma = (*p_up - 2.0 * *p0 + *p_down) / (h * h);

        const double two_h = 2.0 * h;
        if (need(Greek::speed) && std::isfinite(two_h) && spot > two_h && std::isfinite(spot + two_h)) {
            const auto p_up2 = detail::shifted_value(
                engine, option, context, spot + two_h, volatility, rate, valuation_time);
            if (!p_up2) return std::unexpected(p_up2.error());
            const auto p_down2 = detail::shifted_value(
                engine, option, context, spot - two_h, volatility, rate, valuation_time);
            if (!p_down2) return std::unexpected(p_down2.error());
            speed = (*p_up2 - 2.0 * *p_up + 2.0 * *p_down - *p_down2) /
                    (2.0 * h * h * h);
        }
    }

    auto vega = *native.get(Greek::vega);
    auto vanna = *native.get(Greek::vanna);
    auto zomma = *native.get(Greek::zomma);
    const double vol_scale = 100.0 * settings.volatility_shift;
    const bool volatility_stencil_available =
        volatility > settings.volatility_shift &&
        volatility + settings.volatility_shift > volatility &&
        volatility - settings.volatility_shift < volatility &&
        std::isfinite(volatility + settings.volatility_shift) && std::isfinite(vol_scale);
    if (volatility_stencil_available &&
        (need(Greek::vega) || need(Greek::vanna) || need(Greek::zomma))) {
        const double volatility_high = volatility + settings.volatility_shift;
        const double volatility_low = volatility - settings.volatility_shift;
        const auto v_up = detail::shifted_value(
            engine, option, context, spot, volatility_high, rate, valuation_time);
        if (!v_up) return std::unexpected(v_up.error());
        const auto v_down = detail::shifted_value(
            engine, option, context, spot, volatility_low, rate, valuation_time);
        if (!v_down) return std::unexpected(v_down.error());
        if (need(Greek::vega)) vega = (*v_up - *v_down) / (2.0 * vol_scale);

        if (spot_stencil_available && (need(Greek::vanna) || need(Greek::zomma))) {
            const auto d_up = detail::shifted_value(
                engine, option, context, spot + h, volatility_high, rate, valuation_time);
            if (!d_up) return std::unexpected(d_up.error());
            const auto d_down = detail::shifted_value(
                engine, option, context, spot - h, volatility_high, rate, valuation_time);
            if (!d_down) return std::unexpected(d_down.error());
            const auto d_up_low = detail::shifted_value(
                engine, option, context, spot + h, volatility_low, rate, valuation_time);
            if (!d_up_low) return std::unexpected(d_up_low.error());
            const auto d_down_low = detail::shifted_value(
                engine, option, context, spot - h, volatility_low, rate, valuation_time);
            if (!d_down_low) return std::unexpected(d_down_low.error());
            if (need(Greek::vanna)) vanna = ((*d_up - *d_down) - (*d_up_low - *d_down_low)) /
                                            (4.0 * h * vol_scale);

            const double gamma_high = (*d_up - 2.0 * *v_up + *d_down) / (h * h);
            const double gamma_low = (*d_up_low - 2.0 * *v_down + *d_down_low) / (h * h);
            if (need(Greek::zomma)) zomma = (gamma_high - gamma_low) / (2.0 * vol_scale);
        }
    }

    auto rho = *native.get(Greek::rho);
    const double rate_scale = 200.0 * settings.rate_shift;
    if (need(Greek::rho) && std::isfinite(rate + settings.rate_shift) &&
        rate + settings.rate_shift > rate && rate - settings.rate_shift < rate &&
        std::isfinite(rate - settings.rate_shift) && std::isfinite(rate_scale)) {
        const auto r_up = detail::shifted_value(
            engine, option, context, spot, volatility, rate + settings.rate_shift, valuation_time);
        if (!r_up) return std::unexpected(r_up.error());
        const auto r_down = detail::shifted_value(
            engine, option, context, spot, volatility, rate - settings.rate_shift, valuation_time);
        if (!r_down) return std::unexpected(r_down.error());
        rho = (*r_up - *r_down) / rate_scale;
    }

    auto theta = *native.get(Greek::theta);
    auto charm = *native.get(Greek::charm);
    auto color = *native.get(Greek::color);
    if (need(Greek::theta) || need(Greek::charm) || need(Greek::color)) {
        using Days = std::chrono::duration<double, std::ratio<86400>>;
        const auto valuation_days = Days{valuation_time.time_since_epoch()};
        auto lower_days = Days{Timestamp::min().time_since_epoch()};
        auto upper_days = Days{Timestamp::max().time_since_epoch()};
        if constexpr (requires { option.effective_date(); })
            lower_days = Days{option.effective_date().time_since_epoch()};
        if constexpr (requires { option.expiry_date(); })
            upper_days = Days{option.expiry_date().time_since_epoch()};
        const double before_days = std::min<double>(settings.time_shift_days, (valuation_days - lower_days).count());
        const double after_days = std::min<double>(settings.time_shift_days, (upper_days - valuation_days).count());
        const Timestamp before = valuation_time - std::chrono::duration_cast<Timestamp::duration>(Days{before_days});
        const Timestamp after = valuation_time + std::chrono::duration_cast<Timestamp::duration>(Days{after_days});
        bool time_stencil_available = true; // NOLINT(misc-const-correctness): later checks depend on the option type.
        if constexpr (requires { option.averaging_start_date(); option.realized_average(); }) {
            const Timestamp averaging_start = start_of_day(option.averaging_start_date());
            time_stencil_available = option.realized_average() == 0.0
                                         ? after <= averaging_start
                                         : before > averaging_start;
        }
        const auto crosses_event = [&](Date date) {
            const Timestamp event = start_of_day(date);
            return before <= event && event <= after;
        };
        if constexpr (requires { option.barrier_terms(); }) {
            const auto& terms = option.barrier_terms();
            time_stencil_available = time_stencil_available && terms.was_touched_before(before).has_value() &&
                                     terms.was_touched_before(after).has_value();
            if (!terms.is_continuous())
                time_stencil_available = time_stencil_available &&
                                         std::none_of(terms.observation_dates().begin(), terms.observation_dates().end(), crosses_event);
        }
        if constexpr (requires { option.observation_dates(); option.barrier_state(); })
            time_stencil_available = time_stencil_available &&
                                     std::none_of(option.observation_dates().begin(), option.observation_dates().end(), crosses_event);
        if constexpr (requires { option.accumulated_quantity(); } || requires { option.knock_in_observation_mode(); }) {
            bool daily_events = true; // NOLINT(misc-const-correctness): daily monitoring depends on the option type.
            if constexpr (requires { option.knock_in_observation_mode(); })
                daily_events = option.knock_in_observation_mode() == KnockInObservationMode::every_trading_day;
            if (daily_events)
                for (Date date = date_of(before); date <= date_of(after); date += std::chrono::days{1})
                    if (crosses_event(date) && context.calendar().is_trading_day(date)) {
                        time_stencil_available = false;
                        break;
                    }
        }
        if (!spot_discontinuity && time_stencil_available &&
            (need(Greek::theta) || need(Greek::charm) || need(Greek::color)) &&
            (before_days != 0.0 || after_days != 0.0)) {
            const auto t_before = before_days == 0.0
                                      ? p0
                                      : detail::shifted_value(
                                            engine, option, context, spot, volatility, rate, before);
            if (!t_before) return std::unexpected(t_before.error());
            const auto t_after = after_days == 0.0
                                     ? p0
                                     : detail::shifted_value(
                                           engine, option, context, spot, volatility, rate, after);
            if (!t_after) return std::unexpected(t_after.error());
            const double day_scale = before_days + after_days;
            if (need(Greek::theta)) theta = (*t_after - *t_before) / day_scale;

            if (spot_stencil_available && (need(Greek::charm) || need(Greek::color))) {
                const auto d_before = detail::shifted_value(
                    engine, option, context, spot + h, volatility, rate, before);
                if (!d_before) return std::unexpected(d_before.error());
                const auto d_before_low = detail::shifted_value(
                    engine, option, context, spot - h, volatility, rate, before);
                if (!d_before_low) return std::unexpected(d_before_low.error());
                const auto d_after = detail::shifted_value(
                    engine, option, context, spot + h, volatility, rate, after);
                if (!d_after) return std::unexpected(d_after.error());
                const auto d_after_low = detail::shifted_value(
                    engine, option, context, spot - h, volatility, rate, after);
                if (!d_after_low) return std::unexpected(d_after_low.error());
                if (need(Greek::charm)) charm = ((*d_after - *d_after_low) - (*d_before - *d_before_low)) /
                                                (2.0 * h * day_scale);
                if (need(Greek::color)) color = (((*d_after - 2.0 * *t_after + *d_after_low) -
                                                  (*d_before - 2.0 * *t_before + *d_before_low)) /
                                                 (h * h * day_scale));
            }
        }
    }
    auto output = make_pricing_result(*p0, {{Greek::delta, delta}, {Greek::gamma, gamma}, {Greek::speed, speed}, {Greek::theta, theta}, {Greek::charm, charm}, {Greek::color, color}, {Greek::vega, vega}, {Greek::vanna, vanna}, {Greek::zomma, zomma}, {Greek::rho, rho}});
    if (!output) return std::unexpected(output.error());
    *output = output->selected(greeks);
    if (!output->all_finite())
        return std::unexpected(Error{ErrorCategory::invalid_result,
                                     "numerical analytics are non-finite"});
    return output;
}

// Stabilize stochastic engines per request without mutating their stored settings.
template <typename Engine, typename Option, typename Native>
Result<PricingResult> price_with_greeks(
    const Engine& engine, const Option& option, const PricingContext& context,
    GreeksRequest greeks, NumericalShiftSettings settings, const Native& evaluate_native,
    bool native_complete = false)
{
    const auto request_valid = greeks.validate();
    if (!request_valid) return std::unexpected(request_valid.error());
    const auto valid = validate_greeks_settings(settings);
    if (!valid) return std::unexpected(valid.error());
    if constexpr (requires { engine.settings().seed; }) {
        auto simulation = engine.settings();
        if (!simulation.seed) {
            simulation.seed = std::random_device{}();
            return price_with_greeks(Engine{simulation}, option, context, greeks, settings,
                                     evaluate_native, native_complete);
        }
    }
    auto native = evaluate_native(engine);
    const auto value = price_value(native);
    if (!value) return std::unexpected(value.error());
    *native = native->selected(greeks);
    if (!native->all_finite())
        return std::unexpected(Error{ErrorCategory::invalid_result, "native Greeks are non-finite"});
    if (greeks_unavailable(option, context))
        return make_pricing_result(*value);
    if (native_complete) return native;
    return complete_greeks(engine, option, context, greeks, settings, *native);
}

} // namespace detail

/// Computes price and all feasible Greeks using only numerical price differences.
/// At expiry or a monitored barrier hit-state boundary, only price is available.
/// Missing legal stencils leave individual measures empty; a failed feasible valuation
/// fails the operation. Monte Carlo valuations share one seed per request.
/// Shifts and units follow NumericalShiftSettings and Greek, respectively.
template <typename Engine, typename Option>
[[nodiscard]] Result<PricingResult> calculate_numerical_greeks(
    const Engine& engine, const Option& option, const PricingContext& context,
    NumericalShiftSettings settings = {})
{
    return detail::price_with_greeks(engine, option, context, true, settings,
                                     [&](const auto& seeded_engine) -> Result<PricingResult> {
                                         const auto value = detail::numerical_value(seeded_engine, option, context);
                                         if (!value) return std::unexpected(value.error());
                                         return make_pricing_result(*value);
                                     });
}

} // namespace kiyosi
