#pragma once

#include <algorithm>
#include <chrono>
#include <cmath>
#include <ratio>

#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/detail/revaluation.hpp>
#include <kiyosi/pricing/result.hpp>
#include <kiyosi/pricing/settings/numerical_shift.hpp>

namespace kiyosi {

/// Derives the full risk-measure set for any engine by revaluing it on bumped market states.
/// Time shifts are clamped to the instrument life, so boundary valuations use a one-sided step.
template <typename Engine, typename Option>
[[nodiscard]] result<PricingResult> numerical_analytics(
    const Engine& engine, const Option& option, const PricingContext& context,
    NumericalShiftSettings settings = {})
{
    if (!std::isfinite(settings.spot_shift) || settings.spot_shift <= 0.0 ||
        !std::isfinite(settings.volatility_shift) || settings.volatility_shift <= 0.0 ||
        !std::isfinite(settings.rate_shift) || settings.rate_shift <= 0.0 ||
        settings.time_shift_days <= 0)
        return std::unexpected(Error{error_category::invalid_parameter, "numerical shifts are invalid"});

    const double spot = context.asset_price();
    const double volatility = context.parameters().volatility();
    const double rate = context.parameters().risk_free_rate();
    const timestamp today = context.valuation_time();
    const auto p0 = detail::numerical_value(engine, option, context);
    if (!p0) return std::unexpected(p0.error());
    const auto p_up = detail::shifted_value(engine, option, context, spot + settings.spot_shift,
                                            volatility, rate, today);
    const auto p_down = detail::shifted_value(engine, option, context, spot - settings.spot_shift,
                                              volatility, rate, today);
    if (!p_up) return std::unexpected(p_up.error());
    if (!p_down) return std::unexpected(p_down.error());
    const double h = settings.spot_shift;
    const double delta = (*p_up - *p_down) / (2.0 * h);
    const double gamma = (*p_up - 2.0 * *p0 + *p_down) / (h * h);

    const auto p_up2 = detail::shifted_value(engine, option, context, spot + 2.0 * h, volatility, rate, today);
    const auto p_down2 = detail::shifted_value(engine, option, context, spot - 2.0 * h, volatility, rate, today);
    if (!p_up2) return std::unexpected(p_up2.error());
    if (!p_down2) return std::unexpected(p_down2.error());
    const double speed = (*p_up2 - 2.0 * *p_up + 2.0 * *p_down - *p_down2) / (2.0 * h * h * h);

    const auto v_up = detail::shifted_value(engine, option, context, spot,
                                            volatility + settings.volatility_shift, rate, today);
    const auto v_down = detail::shifted_value(engine, option, context, spot,
                                              volatility - settings.volatility_shift, rate, today);
    if (!v_up) return std::unexpected(v_up.error());
    if (!v_down) return std::unexpected(v_down.error());
    const auto d_up = detail::shifted_value(engine, option, context, spot + h,
                                            volatility + settings.volatility_shift, rate, today);
    const auto d_down = detail::shifted_value(engine, option, context, spot - h,
                                              volatility + settings.volatility_shift, rate, today);
    const auto d_up_low = detail::shifted_value(engine, option, context, spot + h,
                                                volatility - settings.volatility_shift, rate, today);
    const auto d_down_low = detail::shifted_value(engine, option, context, spot - h,
                                                  volatility - settings.volatility_shift, rate, today);
    if (!d_up || !d_down || !d_up_low || !d_down_low)
        return std::unexpected(Error{error_category::invalid_result, "numerical analytics shift failed"});
    const double vol_scale = 100.0 * settings.volatility_shift;
    const double vega = (*v_up - *v_down) / (2.0 * vol_scale);
    const double vanna = ((*d_up - *d_down) - (*d_up_low - *d_down_low)) / (4.0 * h * vol_scale);

    const double gamma_high = (*d_up - 2.0 * *v_up + *d_down) / (h * h);
    const double gamma_low = (*d_up_low - 2.0 * *v_down + *d_down_low) / (h * h);
    const double zomma = (gamma_high - gamma_low) / (2.0 * vol_scale);

    const auto r_up = detail::shifted_value(engine, option, context, spot, volatility,
                                            rate + settings.rate_shift, today);
    const auto r_down = detail::shifted_value(engine, option, context, spot, volatility,
                                              rate - settings.rate_shift, today);
    if (!r_up) return std::unexpected(r_up.error());
    if (!r_down) return std::unexpected(r_down.error());
    const double rho = (*r_up - *r_down) / (200.0 * settings.rate_shift);

    timestamp before = today - std::chrono::days{settings.time_shift_days};
    timestamp after = today + std::chrono::days{settings.time_shift_days};
    if constexpr (requires { option.effective(); }) before = std::max(before, start_of_day(option.effective()));
    if constexpr (requires { option.expiry(); }) after = std::min(after, start_of_day(option.expiry()));
    const double before_days = std::chrono::duration<double, std::ratio<86400>>{today - before}.count();
    const double after_days = std::chrono::duration<double, std::ratio<86400>>{after - today}.count();
    if (before_days == 0.0 && after_days == 0.0)
        return std::unexpected(Error{error_category::invalid_result,
                                     "time shifts are unavailable at the boundary"});
    const auto t_before = detail::shifted_value(engine, option, context, spot, volatility, rate, before);
    const auto t_after = detail::shifted_value(engine, option, context, spot, volatility, rate, after);
    if (!t_before || !t_after)
        return std::unexpected(Error{error_category::invalid_result, "time shift failed"});
    const double day_scale = before_days + after_days;
    const double theta = (*t_after - *t_before) / day_scale;

    const auto d_before = detail::shifted_value(engine, option, context, spot + h, volatility, rate, before);
    const auto d_before_low = detail::shifted_value(engine, option, context, spot - h, volatility, rate, before);
    const auto d_after = detail::shifted_value(engine, option, context, spot + h, volatility, rate, after);
    const auto d_after_low = detail::shifted_value(engine, option, context, spot - h, volatility, rate, after);
    if (!d_before || !d_before_low || !d_after || !d_after_low)
        return std::unexpected(Error{error_category::invalid_result, "time shift failed"});
    const double charm = ((*d_after - *d_after_low) - (*d_before - *d_before_low)) / (2.0 * h * day_scale);
    const double color = (((*d_after - 2.0 * *t_after + *d_after_low) -
                           (*d_before - 2.0 * *t_before + *d_before_low)) /
                          (h * h * day_scale));

    PricingResult output{{risk_measure::price, *p0}, {risk_measure::delta, delta},
                         {risk_measure::gamma, gamma}, {risk_measure::speed, speed},
                         {risk_measure::theta, theta}, {risk_measure::charm, charm},
                         {risk_measure::color, color}, {risk_measure::vega, vega},
                         {risk_measure::vanna, vanna}, {risk_measure::zomma, zomma},
                         {risk_measure::rho, rho}};
    if (!output.all_finite())
        return std::unexpected(Error{error_category::invalid_result,
                                     "numerical analytics are non-finite"});
    return output;
}

template <typename Engine, typename Option>
[[nodiscard]] result<PricingResult> numerical_price(
    const Engine& engine, const Option& option, const PricingContext& context,
    NumericalShiftSettings settings = {})
{
    return numerical_analytics(engine, option, context, settings);
}

} // namespace kiyosi
