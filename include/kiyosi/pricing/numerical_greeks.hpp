#pragma once

#include <algorithm>
#include <chrono>
#include <cmath>
#include <concepts>
#include <optional>
#include <ratio>
#include <random>

#include <kiyosi/instruments/barrier/terms.hpp>
#include <kiyosi/instruments/structured/autocallable.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>
#include <kiyosi/pricing/settings/finite_difference.hpp>
#include <kiyosi/pricing/settings/numerical_shift.hpp>

namespace kiyosi {

namespace detail {

template <typename Engine, typename Option>
[[nodiscard]] Result<double> numerical_value(
    const Engine& engine, const Option& option, const PricingContext& context)
{
    auto priced = engine.price(option, context);
    if (!priced) return std::unexpected(priced.error());
    return checked_price(*priced);
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
    return checked_price(result->price());
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

// Use the rounded upper bump on both sides; a collapsed or asymmetric stencil is unavailable.
inline double symmetric_shift(double value, double requested)
{
    const double high = value + requested;
    const double shift = high - value;
    const double low = value - shift;
    return std::isfinite(high) && std::isfinite(shift) && shift > 0.0 &&
                   std::isfinite(low) && value - low == shift
               ? shift
               : 0.0;
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
bool at_spot_discontinuity(const Option& option, const PricingContext& context, double radius = 0.0)
{
    const double spot = context.spot_price();
    const auto crosses = [&](double level) { return spot - radius <= level && level <= spot + radius; };
    if constexpr (requires { option.barrier_terms(); })
        return option.barrier_terms().touch_state() != BarrierTouchState::touched &&
               option.barrier_terms().is_monitored_at(context.valuation_time()) && crosses(option.barrier_level());
    if (context.valuation_time() != start_of_day(context.valuation_date())) return false;
    if constexpr (requires { option.accumulated_quantity(); }) {
        if (context.calendar().is_trading_day(context.valuation_date()))
            return crosses(option.knock_out_level()) ||
                   (spot < option.knock_out_level() && crosses(option.strike()) &&
                    option.acceleration_factor() != 1.0);
    }
    if constexpr (requires { option.knock_out_levels(); option.barrier_state(); }) {
        if (option.barrier_state() == AutocallableBarrierState::knocked_out) return false;
        for (std::size_t index = 0; index < option.observation_dates().size(); ++index) {
            if (option.observation_dates()[index] != context.valuation_date()) continue;
            if (crosses(option.knock_out_levels()[index])) return true;
            if (spot > option.knock_out_levels()[index]) return false;
            if constexpr (requires { option.coupon_barrier_levels(); })
                if (option.coupon_rate() != 0.0 && crosses(option.coupon_barrier_levels()[index]))
                    return true;
        }
        if constexpr (requires { option.knock_in_level(); })
            // Native pricing has already validated history; absent initial history
            // therefore means not knocked in, just as in the pricing engines.
            return option.barrier_state() != AutocallableBarrierState::knocked_in &&
                   option.knock_in_observation_mode() == KnockInObservationMode::every_trading_day &&
                   context.calendar().is_trading_day(context.valuation_date()) &&
                   crosses(option.knock_in_level());
    }
    return false;
}

// Enforces boundary availability, then fills only missing requested measures.
// Away from unavailable boundaries, a supplied native value is never overwritten.
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
    const double h = symmetric_shift(spot, settings.spot_shift);
    const bool spot_discontinuity = at_spot_discontinuity(option, context);
    const bool spot_stencil_available = h > 0.0 && !at_spot_discontinuity(option, context, h) && spot > h &&
                                        std::isfinite(spot + h) && spot + h > spot && spot - h < spot;
    auto delta = *native.get(Greek::delta);
    auto gamma = *native.get(Greek::gamma);
    auto speed = *native.get(Greek::speed);
    std::optional<double> spot_up;
    std::optional<double> spot_down;
    if (spot_stencil_available && (need(Greek::delta) || need(Greek::gamma) || need(Greek::speed))) {
        const auto p_up = detail::shifted_value(
            engine, option, context, spot + h, volatility, rate, valuation_time);
        if (!p_up) return std::unexpected(p_up.error());
        const auto p_down = detail::shifted_value(
            engine, option, context, spot - h, volatility, rate, valuation_time);
        if (!p_down) return std::unexpected(p_down.error());
        spot_up = *p_up;
        spot_down = *p_down;
        if (need(Greek::delta)) delta = (*p_up - *p_down) / (2.0 * h);
        // Divide separately so powers of the shift cannot overflow or underflow.
        if (need(Greek::gamma)) gamma = (*p_up - 2.0 * *p0 + *p_down) / h / h;

        const double two_h = 2.0 * h;
        if (need(Greek::speed) && std::isfinite(two_h) && spot > two_h && symmetric_shift(spot, two_h) == two_h &&
            !at_spot_discontinuity(option, context, two_h)) {
            const auto p_up2 = detail::shifted_value(
                engine, option, context, spot + two_h, volatility, rate, valuation_time);
            if (!p_up2) return std::unexpected(p_up2.error());
            const auto p_down2 = detail::shifted_value(
                engine, option, context, spot - two_h, volatility, rate, valuation_time);
            if (!p_down2) return std::unexpected(p_down2.error());
            speed = (*p_up2 - 2.0 * *p_up + 2.0 * *p_down - *p_down2) /
                    h / h / two_h;
        }
    }

    auto vega = *native.get(Greek::vega);
    auto vanna = *native.get(Greek::vanna);
    auto zomma = *native.get(Greek::zomma);
    const double volatility_shift = symmetric_shift(volatility, settings.volatility_shift);
    const double vol_scale = 100.0 * volatility_shift;
    const bool volatility_stencil_available =
        volatility_shift > 0.0 && volatility > volatility_shift && std::isfinite(vol_scale);
    if (volatility_stencil_available &&
        (need(Greek::vega) || (spot_stencil_available && (need(Greek::vanna) || need(Greek::zomma))))) {
        const double volatility_high = volatility + volatility_shift;
        const double volatility_low = volatility - volatility_shift;
        const bool need_volatility_prices = need(Greek::vega) || need(Greek::zomma);
        const auto v_up = need_volatility_prices ? detail::shifted_value(
                                                       engine, option, context, spot, volatility_high, rate, valuation_time)
                                                 : p0;
        if (!v_up) return std::unexpected(v_up.error());
        const auto v_down = need_volatility_prices ? detail::shifted_value(
                                                         engine, option, context, spot, volatility_low, rate, valuation_time)
                                                   : p0;
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

            if (need(Greek::zomma)) {
                const double gamma_high = (*d_up - 2.0 * *v_up + *d_down) / h / h;
                const double gamma_low = (*d_up_low - 2.0 * *v_down + *d_down_low) / h / h;
                zomma = (gamma_high - gamma_low) / (2.0 * vol_scale);
            }
        }
    }

    auto rho = *native.get(Greek::rho);
    const double rate_shift = symmetric_shift(rate, settings.rate_shift);
    const double rate_scale = 200.0 * rate_shift;
    if (need(Greek::rho) && rate_shift > 0.0 && std::isfinite(rate_scale)) {
        const auto r_up = detail::shifted_value(
            engine, option, context, spot, volatility, rate + rate_shift, valuation_time);
        if (!r_up) return std::unexpected(r_up.error());
        const auto r_down = detail::shifted_value(
            engine, option, context, spot, volatility, rate - rate_shift, valuation_time);
        if (!r_down) return std::unexpected(r_down.error());
        rho = (*r_up - *r_down) / rate_scale;
    }

    auto theta = *native.get(Greek::theta);
    auto charm = *native.get(Greek::charm);
    auto color = *native.get(Greek::color);
    if (need(Greek::theta) || need(Greek::charm) || need(Greek::color)) {
        using Days = std::chrono::duration<double, std::ratio<86400>>;
        auto lower_bound = start_of_day(Date{std::chrono::year::min() / std::chrono::January / 1});
        auto upper_bound = start_of_day(Date{std::chrono::year::max() / std::chrono::December / 31}) +
                           std::chrono::days{1} - std::chrono::microseconds{1};
        if constexpr (requires { option.effective_date(); })
            lower_bound = start_of_day(option.effective_date());
        if constexpr (requires { option.expiry_date(); })
            upper_bound = start_of_day(option.expiry_date());
        // Bound whole days before conversion so even INT_MAX shifts fit in microseconds.
        const auto requested_shift = std::chrono::duration_cast<Timestamp::duration>(
            std::min(std::chrono::days{settings.time_shift_days}, std::chrono::floor<std::chrono::days>(upper_bound - lower_bound)));
        const auto available_before = valuation_time - lower_bound;
        const auto available_after = upper_bound - valuation_time;
        const auto shift = available_before > Timestamp::duration::zero() && available_after > Timestamp::duration::zero()
                               ? std::min({requested_shift, available_before, available_after})
                               : requested_shift;
        const Timestamp before = valuation_time - std::min(shift, available_before);
        const Timestamp after = valuation_time + std::min(shift, available_after);
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
        if (!spot_discontinuity && time_stencil_available && before != after) {
            const bool need_time_prices = need(Greek::theta) || (spot_stencil_available && need(Greek::color));
            const auto t_before = !need_time_prices || before == valuation_time
                                      ? p0
                                      : detail::shifted_value(
                                            engine, option, context, spot, volatility, rate, before);
            if (!t_before) return std::unexpected(t_before.error());
            const auto t_after = !need_time_prices || after == valuation_time
                                     ? p0
                                     : detail::shifted_value(
                                           engine, option, context, spot, volatility, rate, after);
            if (!t_after) return std::unexpected(t_after.error());
            const double day_scale = Days{after - before}.count();
            if (need(Greek::theta)) theta = (*t_after - *t_before) / day_scale;

            if (spot_stencil_available && (need(Greek::charm) || need(Greek::color))) {
                const auto d_before = before == valuation_time && spot_up
                                          ? Result<double>{*spot_up}
                                          : detail::shifted_value(
                                                engine, option, context, spot + h, volatility, rate, before);
                if (!d_before) return std::unexpected(d_before.error());
                const auto d_before_low = before == valuation_time && spot_down
                                              ? Result<double>{*spot_down}
                                              : detail::shifted_value(
                                                    engine, option, context, spot - h, volatility, rate, before);
                if (!d_before_low) return std::unexpected(d_before_low.error());
                const auto d_after = after == valuation_time && spot_up
                                         ? Result<double>{*spot_up}
                                         : detail::shifted_value(
                                               engine, option, context, spot + h, volatility, rate, after);
                if (!d_after) return std::unexpected(d_after.error());
                const auto d_after_low = after == valuation_time && spot_down
                                             ? Result<double>{*spot_down}
                                             : detail::shifted_value(
                                                   engine, option, context, spot - h, volatility, rate, after);
                if (!d_after_low) return std::unexpected(d_after_low.error());
                if (need(Greek::charm)) charm = ((*d_after - *d_after_low) - (*d_before - *d_before_low)) /
                                                (2.0 * h * day_scale);
                if (need(Greek::color)) color = (((*d_after - 2.0 * *t_after + *d_after_low) -
                                                  (*d_before - 2.0 * *t_before + *d_before_low)) /
                                                 h / h / day_scale);
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

// Stabilize simulation seeds and finite-difference domains without mutating stored settings.
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
    if (native_complete) {
        if (greeks_unavailable(option, context))
            return make_pricing_result(*value);
        return native;
    }
    if constexpr (requires { { engine.settings() } -> std::same_as<FiniteDifferenceSettings>; }) {
        auto grid = engine.settings();
        if (!grid.asset_upper_boundary) {
            // Spot bumps must move through one fixed mesh, rather than moving the mesh with spot.
            grid.asset_upper_boundary = default_finite_difference_upper_boundary(option, context);
            // Preserve settlements that never construct an asset grid.
            if (std::isfinite(*grid.asset_upper_boundary))
                return complete_greeks(Engine{grid}, option, context, greeks, settings, *native);
        }
    }
    return complete_greeks(engine, option, context, greeks, settings, *native);
}

template <typename Engine, typename Option>
Result<PricingResult> price_with_greeks(
    const Engine& engine, const Option& option, const PricingContext& context,
    GreeksRequest greeks, NumericalShiftSettings settings)
{
    return price_with_greeks(engine, option, context, greeks, settings,
                             [&](const auto& seeded_engine) -> Result<PricingResult> {
                                 const auto value = numerical_value(seeded_engine, option, context);
                                 if (!value) return std::unexpected(value.error());
                                 return make_pricing_result(*value);
                             });
}

} // namespace detail

/// Computes price and all feasible Greeks using only numerical price differences.
/// At expiry or a monitored barrier hit-state boundary, only price is available.
/// Missing legal stencils leave individual measures empty; a failed feasible valuation
/// fails the operation. Monte Carlo valuations share one seed per request.
/// Spot stencils crossing a currently monitored event threshold are unavailable.
/// Finite-difference valuations share the unshifted context's asset domain.
/// Shifts and units follow NumericalShiftSettings and Greek, respectively.
template <typename Engine, typename Option>
[[nodiscard]] Result<PricingResult> calculate_numerical_greeks(
    const Engine& engine, const Option& option, const PricingContext& context,
    NumericalShiftSettings settings = {})
{
    return detail::price_with_greeks(engine, option, context, true, settings);
}

} // namespace kiyosi
