#pragma once

#include <algorithm>
#include <chrono>
#include <cmath>
#include <optional>
#include <ratio>
#include <random>

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
    const auto value = result->require(RiskMeasure::price);
    if (!value) return std::unexpected(value.error());
    if (!std::isfinite(*value))
        return std::unexpected(Error{ErrorCategory::invalid_result, "pricing produced no finite price"});
    return value;
}

inline Result<void> validate_greeks_settings(GreeksLevel level, NumericalShiftSettings settings)
{
    if (level != GreeksLevel::basic && level != GreeksLevel::full)
        return std::unexpected(Error{ErrorCategory::invalid_parameter, "unknown Greeks level"});
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
        if (option.barrier_terms().is_monitored_on(context.valuation_date()) &&
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
            return option.barrier_state() == AutocallableBarrierState::none &&
                   option.knock_in_observation_mode() == KnockInObservationMode::every_trading_day &&
                   spot == option.knock_in_level();
    }
    return false;
}

// Fills only missing requested measures. A supplied native value is never overwritten.
template <typename Engine, typename Option>
Result<PricingResult> complete_greeks(
    const Engine& engine, const Option& option, const PricingContext& context,
    GreeksLevel level, NumericalShiftSettings settings, const PricingResult& native)
{
    const double spot = context.spot_price();
    const double volatility = context.model_parameters().volatility();
    const double rate = context.model_parameters().risk_free_rate();
    const Timestamp valuation_time = context.valuation_time();
    const auto p0 = native.require(RiskMeasure::price);
    if (!p0) return std::unexpected(p0.error());
    if (greeks_unavailable(option, context))
        return make_pricing_result({{RiskMeasure::price, *p0}});
    const auto need = [&](RiskMeasure measure) {
        return !native.has(measure) && (level == GreeksLevel::full ||
            measure == RiskMeasure::delta || measure == RiskMeasure::gamma);
    };
    const double h = settings.spot_shift;
    const bool spot_discontinuity = at_spot_discontinuity(option, context);
    const bool spot_stencil_available = !spot_discontinuity && spot > h &&
        std::isfinite(spot + h) && spot + h > spot && spot - h < spot;
    auto delta = *native.get(RiskMeasure::delta);
    auto gamma = *native.get(RiskMeasure::gamma);
    auto speed = *native.get(RiskMeasure::speed);
    if (spot_stencil_available && (need(RiskMeasure::delta) || need(RiskMeasure::gamma) || need(RiskMeasure::speed))) {
        const auto p_up = detail::shifted_value(
            engine, option, context, spot + h, volatility, rate, valuation_time);
        if (!p_up) return std::unexpected(p_up.error());
        const auto p_down = detail::shifted_value(
            engine, option, context, spot - h, volatility, rate, valuation_time);
        if (!p_down) return std::unexpected(p_down.error());
        if (need(RiskMeasure::delta)) delta = (*p_up - *p_down) / (2.0 * h);
        if (need(RiskMeasure::gamma)) gamma = (*p_up - 2.0 * *p0 + *p_down) / (h * h);

        const double two_h = 2.0 * h;
        if (need(RiskMeasure::speed) && std::isfinite(two_h) && spot > two_h && std::isfinite(spot + two_h)) {
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

    if (level == GreeksLevel::basic) {
        auto output = make_pricing_result({{RiskMeasure::price, *p0},
            {RiskMeasure::delta, delta}, {RiskMeasure::gamma, gamma}});
        if (output && !output->all_finite())
            return std::unexpected(Error{ErrorCategory::invalid_result, "numerical analytics are non-finite"});
        return output;
    }

    auto vega = *native.get(RiskMeasure::vega);
    auto vanna = *native.get(RiskMeasure::vanna);
    auto zomma = *native.get(RiskMeasure::zomma);
    const double vol_scale = 100.0 * settings.volatility_shift;
    const bool volatility_stencil_available =
        volatility > settings.volatility_shift &&
        std::isfinite(volatility + settings.volatility_shift) && std::isfinite(vol_scale);
    if (volatility_stencil_available &&
        (need(RiskMeasure::vega) || need(RiskMeasure::vanna) || need(RiskMeasure::zomma))) {
        const double volatility_high = volatility + settings.volatility_shift;
        const double volatility_low = volatility - settings.volatility_shift;
        const auto v_up = detail::shifted_value(
            engine, option, context, spot, volatility_high, rate, valuation_time);
        if (!v_up) return std::unexpected(v_up.error());
        const auto v_down = detail::shifted_value(
            engine, option, context, spot, volatility_low, rate, valuation_time);
        if (!v_down) return std::unexpected(v_down.error());
        if (need(RiskMeasure::vega)) vega = (*v_up - *v_down) / (2.0 * vol_scale);

        if (spot_stencil_available && (need(RiskMeasure::vanna) || need(RiskMeasure::zomma))) {
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
            if (need(RiskMeasure::vanna)) vanna = ((*d_up - *d_down) - (*d_up_low - *d_down_low)) /
                    (4.0 * h * vol_scale);

            const double gamma_high = (*d_up - 2.0 * *v_up + *d_down) / (h * h);
            const double gamma_low = (*d_up_low - 2.0 * *v_down + *d_down_low) / (h * h);
            if (need(RiskMeasure::zomma)) zomma = (gamma_high - gamma_low) / (2.0 * vol_scale);
        }
    }

    auto rho = *native.get(RiskMeasure::rho);
    const double rate_scale = 200.0 * settings.rate_shift;
    if (need(RiskMeasure::rho) && std::isfinite(rate + settings.rate_shift) &&
        std::isfinite(rate - settings.rate_shift) && std::isfinite(rate_scale)) {
        const auto r_up = detail::shifted_value(
            engine, option, context, spot, volatility, rate + settings.rate_shift, valuation_time);
        if (!r_up) return std::unexpected(r_up.error());
        const auto r_down = detail::shifted_value(
            engine, option, context, spot, volatility, rate - settings.rate_shift, valuation_time);
        if (!r_down) return std::unexpected(r_down.error());
        rho = (*r_up - *r_down) / rate_scale;
    }

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
    auto theta = *native.get(RiskMeasure::theta);
    auto charm = *native.get(RiskMeasure::charm);
    auto color = *native.get(RiskMeasure::color);
    bool time_stencil_available = true;
    if constexpr (requires { option.averaging_start_date(); option.realized_average(); }) {
        const Timestamp averaging_start = start_of_day(option.averaging_start_date());
        time_stencil_available = option.realized_average() == 0.0
                                     ? after <= averaging_start : before > averaging_start;
    }
    if (!spot_discontinuity && time_stencil_available &&
        (need(RiskMeasure::theta) || need(RiskMeasure::charm) || need(RiskMeasure::color)) &&
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
        if (need(RiskMeasure::theta)) theta = (*t_after - *t_before) / day_scale;

        if (spot_stencil_available && (need(RiskMeasure::charm) || need(RiskMeasure::color))) {
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
            if (need(RiskMeasure::charm)) charm = ((*d_after - *d_after_low) - (*d_before - *d_before_low)) /
                    (2.0 * h * day_scale);
            if (need(RiskMeasure::color)) color = (((*d_after - 2.0 * *t_after + *d_after_low) -
                      (*d_before - 2.0 * *t_before + *d_before_low)) /
                     (h * h * day_scale));
        }
    }

    auto output = make_pricing_result(
        {{RiskMeasure::price, *p0}, {RiskMeasure::delta, delta}, {RiskMeasure::gamma, gamma}, {RiskMeasure::speed, speed}, {RiskMeasure::theta, theta}, {RiskMeasure::charm, charm}, {RiskMeasure::color, color}, {RiskMeasure::vega, vega}, {RiskMeasure::vanna, vanna}, {RiskMeasure::zomma, zomma}, {RiskMeasure::rho, rho}});
    if (!output) return std::unexpected(output.error());
    if (!output->all_finite())
        return std::unexpected(Error{ErrorCategory::invalid_result,
                                     "numerical analytics are non-finite"});
    return output;
}

// Stabilize stochastic engines per request without mutating their stored settings.
template <typename Engine, typename Option, typename Native>
Result<PricingResult> price_with_greeks(
    const Engine& engine, const Option& option, const PricingContext& context,
    GreeksLevel level, NumericalShiftSettings settings, const Native& evaluate_native,
    bool native_complete = false)
{
    const auto valid = validate_greeks_settings(level, settings);
    if (!valid) return std::unexpected(valid.error());
    if constexpr (requires { engine.settings().seed; }) {
        auto simulation = engine.settings();
        if (!simulation.seed) {
            simulation.seed = std::random_device{}();
            return price_with_greeks(Engine{simulation}, option, context, level, settings,
                                     evaluate_native, native_complete);
        }
    }
    auto native = evaluate_native(engine);
    const auto value = price_value(native);
    if (!value) return std::unexpected(value.error());
    if (!native->all_finite())
        return std::unexpected(Error{ErrorCategory::invalid_result, "native Greeks are non-finite"});
    if (greeks_unavailable(option, context))
        return make_pricing_result({{RiskMeasure::price, *value}});
    if (native_complete) return native;
    return complete_greeks(engine, option, context, level, settings, *native);
}

} // namespace detail

/// Computes price and all feasible Greeks using only numerical price differences.
/// At expiry or a monitored barrier hit-state boundary, only price is available.
/// Missing legal stencils leave individual measures empty; a failed feasible valuation
/// fails the operation. Monte Carlo valuations share one seed per request.
/// Shifts and units follow NumericalShiftSettings and RiskMeasure, respectively.
template <typename Engine, typename Option>
[[nodiscard]] Result<PricingResult> calculate_numerical_risk_measures(
    const Engine& engine, const Option& option, const PricingContext& context,
    NumericalShiftSettings settings = {})
{
    return detail::price_with_greeks(engine, option, context, GreeksLevel::full, settings,
        [&](const auto& seeded_engine) -> Result<PricingResult> {
            const auto value = detail::numerical_value(seeded_engine, option, context);
            if (!value) return std::unexpected(value.error());
            return make_pricing_result({{RiskMeasure::price, *value}});
        });
}

} // namespace kiyosi
