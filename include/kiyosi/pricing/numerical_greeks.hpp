#pragma once

#include <algorithm>
#include <chrono>
#include <cmath>
#include <optional>
#include <ratio>

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
    const auto value = priced->get(RiskMeasure::price);
    if (!value) return std::unexpected(value.error());
    if (!*value || !std::isfinite(**value))
        return std::unexpected(Error{ErrorCategory::invalid_result, "pricing produced no finite price"});
    return **value;
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

} // namespace detail

/// Derives risk measures for any engine by revaluing it on bumped market states.
/// Spot, volatility, and rate shifts are absolute; time shifts are calendar days. Central
/// stencils are used except where time is clamped to the instrument life. Measures with no
/// supported stencil inside a model boundary are unavailable, while the valid base price and
/// independent measures are retained. A failure while pricing any feasible bumped state fails the
/// whole operation.
template <typename Engine, typename Option>
[[nodiscard]] Result<PricingResult> calculate_numerical_analytics(
    const Engine& engine, const Option& option, const PricingContext& context,
    NumericalShiftSettings settings = {})
{
    if (!std::isfinite(settings.spot_shift) || settings.spot_shift <= 0.0 ||
        !std::isfinite(settings.volatility_shift) || settings.volatility_shift <= 0.0 ||
        !std::isfinite(settings.rate_shift) || settings.rate_shift <= 0.0 ||
        settings.time_shift_days <= 0)
        return std::unexpected(Error{ErrorCategory::invalid_parameter, "numerical shifts are invalid"});

    const double spot = context.spot_price();
    const double volatility = context.model_parameters().volatility();
    const double rate = context.model_parameters().risk_free_rate();
    const Timestamp today = context.valuation_time();
    const auto p0 = detail::numerical_value(engine, option, context);
    if (!p0) return std::unexpected(p0.error());
    const double h = settings.spot_shift;
    const bool spot_stencil_available = spot > h && std::isfinite(spot + h);
    std::optional<double> delta;
    std::optional<double> gamma;
    std::optional<double> speed;
    if (spot_stencil_available) {
        const auto p_up = detail::shifted_value(
            engine, option, context, spot + h, volatility, rate, today);
        if (!p_up) return std::unexpected(p_up.error());
        const auto p_down = detail::shifted_value(
            engine, option, context, spot - h, volatility, rate, today);
        if (!p_down) return std::unexpected(p_down.error());
        delta = (*p_up - *p_down) / (2.0 * h);
        gamma = (*p_up - 2.0 * *p0 + *p_down) / (h * h);

        const double two_h = 2.0 * h;
        if (std::isfinite(two_h) && spot > two_h && std::isfinite(spot + two_h)) {
            const auto p_up2 = detail::shifted_value(
                engine, option, context, spot + two_h, volatility, rate, today);
            if (!p_up2) return std::unexpected(p_up2.error());
            const auto p_down2 = detail::shifted_value(
                engine, option, context, spot - two_h, volatility, rate, today);
            if (!p_down2) return std::unexpected(p_down2.error());
            speed = (*p_up2 - 2.0 * *p_up + 2.0 * *p_down - *p_down2) /
                    (2.0 * h * h * h);
        }
    }

    std::optional<double> vega;
    std::optional<double> vanna;
    std::optional<double> zomma;
    const double vol_scale = 100.0 * settings.volatility_shift;
    const bool volatility_stencil_available =
        volatility > settings.volatility_shift &&
        std::isfinite(volatility + settings.volatility_shift) && std::isfinite(vol_scale);
    if (volatility_stencil_available) {
        const double volatility_high = volatility + settings.volatility_shift;
        const double volatility_low = volatility - settings.volatility_shift;
        const auto v_up = detail::shifted_value(
            engine, option, context, spot, volatility_high, rate, today);
        if (!v_up) return std::unexpected(v_up.error());
        const auto v_down = detail::shifted_value(
            engine, option, context, spot, volatility_low, rate, today);
        if (!v_down) return std::unexpected(v_down.error());
        vega = (*v_up - *v_down) / (2.0 * vol_scale);

        if (spot_stencil_available) {
            const auto d_up = detail::shifted_value(
                engine, option, context, spot + h, volatility_high, rate, today);
            if (!d_up) return std::unexpected(d_up.error());
            const auto d_down = detail::shifted_value(
                engine, option, context, spot - h, volatility_high, rate, today);
            if (!d_down) return std::unexpected(d_down.error());
            const auto d_up_low = detail::shifted_value(
                engine, option, context, spot + h, volatility_low, rate, today);
            if (!d_up_low) return std::unexpected(d_up_low.error());
            const auto d_down_low = detail::shifted_value(
                engine, option, context, spot - h, volatility_low, rate, today);
            if (!d_down_low) return std::unexpected(d_down_low.error());
            vanna = ((*d_up - *d_down) - (*d_up_low - *d_down_low)) /
                    (4.0 * h * vol_scale);

            const double gamma_high = (*d_up - 2.0 * *v_up + *d_down) / (h * h);
            const double gamma_low = (*d_up_low - 2.0 * *v_down + *d_down_low) / (h * h);
            zomma = (gamma_high - gamma_low) / (2.0 * vol_scale);
        }
    }

    std::optional<double> rho;
    const double rate_scale = 200.0 * settings.rate_shift;
    if (std::isfinite(rate + settings.rate_shift) &&
        std::isfinite(rate - settings.rate_shift) && std::isfinite(rate_scale)) {
        const auto r_up = detail::shifted_value(
            engine, option, context, spot, volatility, rate + settings.rate_shift, today);
        if (!r_up) return std::unexpected(r_up.error());
        const auto r_down = detail::shifted_value(
            engine, option, context, spot, volatility, rate - settings.rate_shift, today);
        if (!r_down) return std::unexpected(r_down.error());
        rho = (*r_up - *r_down) / rate_scale;
    }

    Timestamp before = today - std::chrono::days{settings.time_shift_days};
    Timestamp after = today + std::chrono::days{settings.time_shift_days};
    if constexpr (requires { option.effective_date(); }) before = std::max(before, start_of_day(option.effective_date()));
    if constexpr (requires { option.expiry_date(); }) after = std::min(after, start_of_day(option.expiry_date()));
    const double before_days = std::chrono::duration<double, std::ratio<86400>>{today - before}.count();
    const double after_days = std::chrono::duration<double, std::ratio<86400>>{after - today}.count();
    std::optional<double> theta;
    std::optional<double> charm;
    std::optional<double> color;
    if (before_days != 0.0 || after_days != 0.0) {
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
        theta = (*t_after - *t_before) / day_scale;

        if (spot_stencil_available) {
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
            charm = ((*d_after - *d_after_low) - (*d_before - *d_before_low)) /
                    (2.0 * h * day_scale);
            color = (((*d_after - 2.0 * *t_after + *d_after_low) -
                      (*d_before - 2.0 * *t_before + *d_before_low)) /
                     (h * h * day_scale));
        }
    }

    auto output = make_pricing_result(
        {{RiskMeasure::price, *p0}, {RiskMeasure::delta, delta},
         {RiskMeasure::gamma, gamma}, {RiskMeasure::speed, speed},
         {RiskMeasure::theta, theta}, {RiskMeasure::charm, charm},
         {RiskMeasure::color, color}, {RiskMeasure::vega, vega},
         {RiskMeasure::vanna, vanna}, {RiskMeasure::zomma, zomma},
         {RiskMeasure::rho, rho}});
    if (!output) return std::unexpected(output.error());
    if (!output->all_finite())
        return std::unexpected(Error{ErrorCategory::invalid_result,
                                     "numerical analytics are non-finite"});
    return output;
}

} // namespace kiyosi
