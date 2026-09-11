#pragma once

#include <algorithm>
#include <chrono>
#include <cmath>
#include <numeric>
#include <span>
#include <utility>
#include <vector>

#include <kiyosi/core/types.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/result.hpp>
#include <kiyosi/pricing/engines/settings/implied_volatility.hpp>

namespace kiyosi {

struct NumericalShiftSettings {
    double spot_shift = 1e-2;
    double volatility_shift = 1e-4;
    double rate_shift = 1e-4;
    int time_shift_days = 1;
};

struct ScenarioGridResult {
    std::vector<double> values;
    std::vector<double> deltas;
    std::vector<double> gammas;
};

struct ImpliedCouponSettings {
    double lower_bound = 0.0;
    double upper_bound = 2.0;
    double tolerance = 1e-8;
    int max_iterations = 100;
};

template <typename Engine, typename Option>
[[nodiscard]] result<PricingResult> numerical_analytics(
    const Engine&, const Option&, const PricingContext&, NumericalShiftSettings);
template <typename Engine, typename Option>
[[nodiscard]] result<ScenarioGridResult> scenario_grid(
    const Engine&, const Option&, const PricingContext&, std::span<const double>, NumericalShiftSettings);
template <typename Engine, typename Option>
[[nodiscard]] result<double> implied_volatility(
    const Engine&, const Option&, const PricingContext&, double, ImpliedVolatilitySettings);
template <typename Engine, typename Option>
[[nodiscard]] result<double> implied_coupon(
    const Engine&, const Option&, const PricingContext&, double, ImpliedCouponSettings);

template <typename Engine>
class NumericalAnalyticsEngine {
public:
    explicit NumericalAnalyticsEngine(Engine engine = {}, NumericalShiftSettings settings = {})
        : engine_(std::move(engine)), settings_(settings) {}

    template <typename Option>
    [[nodiscard]] result<PricingResult> price(const Option& option, const PricingContext& context) const
    { return numerical_analytics(engine_, option, context, settings_); }

    template <typename Option>
    [[nodiscard]] result<ScenarioGridResult> scenario_grid(
        const Option& option, const PricingContext& context, std::span<const double> spots) const
    { return kiyosi::scenario_grid(engine_, option, context, spots, settings_); }

    template <typename Option>
    [[nodiscard]] result<double> implied_volatility(
        const Option& option, const PricingContext& context, double observed_price,
        ImpliedVolatilitySettings settings = {}) const
    { return kiyosi::implied_volatility(engine_, option, context, observed_price, settings); }

    template <typename Option>
    [[nodiscard]] result<double> implied_coupon(
        const Option& option, const PricingContext& context, double observed_price,
        ImpliedCouponSettings settings = {}) const
    requires requires(const Option& value, double coupon) { value.with_coupon_rate(coupon); }
    { return kiyosi::implied_coupon(engine_, option, context, observed_price, settings); }

    const Engine& engine() const noexcept { return engine_; }
    NumericalShiftSettings settings() const noexcept { return settings_; }

private:
    Engine engine_;
    NumericalShiftSettings settings_;
};

namespace detail {
template <typename Engine, typename Option>
result<double> numerical_value(const Engine& engine, const Option& option, const PricingContext& context)
{
    auto priced = engine.price(option, context);
    if (!priced) return std::unexpected(priced.error());
    const auto value = priced->get(risk_measure::price);
    if (!value || !std::isfinite(*value))
        return std::unexpected(Error{error_category::invalid_result, "pricing produced no finite price"});
    return *value;
}

inline result<PricingContext> shifted_context(const PricingContext& context, double spot, double volatility,
                                              double rate, timestamp valuation)
{
    auto parameters = make_bsm_parameters(rate, context.parameters().dividend_yield(), volatility);
    if (!parameters) return std::unexpected(parameters.error());
    auto asset = make_asset_price(spot);
    if (!asset) return std::unexpected(asset.error());
    return make_pricing_context(*parameters, *asset, valuation, context.calendar());
}

template <typename Engine, typename Option>
result<double> shifted_value(const Engine& engine, const Option& option, const PricingContext& context,
                             double spot, double volatility, double rate, timestamp valuation)
{
    auto shifted = shifted_context(context, spot, volatility, rate, valuation);
    if (!shifted) return std::unexpected(shifted.error());
    return numerical_value(engine, option, *shifted);
}
} // namespace detail

template <typename Engine, typename Option>
[[nodiscard]] result<PricingResult> numerical_analytics(
    const Engine& engine, const Option& option, const PricingContext& context,
    NumericalShiftSettings settings = {})
{
    if (!std::isfinite(settings.spot_shift) || settings.spot_shift <= 0.0 ||
        !std::isfinite(settings.volatility_shift) || settings.volatility_shift <= 0.0 ||
        !std::isfinite(settings.rate_shift) || settings.rate_shift <= 0.0 || settings.time_shift_days <= 0)
        return std::unexpected(Error{error_category::invalid_parameter, "numerical shifts are invalid"});

    const double spot = context.asset_price().value();
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
    const double vanna = ((*d_up - *d_down) - (*d_up_low - *d_down_low)) /
                         (4.0 * h * vol_scale);

    const auto g_up = detail::shifted_value(engine, option, context, spot + h,
                                            volatility + settings.volatility_shift, rate, today);
    const auto g_down = detail::shifted_value(engine, option, context, spot - h,
                                              volatility + settings.volatility_shift, rate, today);
    const auto g_up_low = detail::shifted_value(engine, option, context, spot + h,
                                                volatility - settings.volatility_shift, rate, today);
    const auto g_down_low = detail::shifted_value(engine, option, context, spot - h,
                                                  volatility - settings.volatility_shift, rate, today);
    if (!g_up || !g_down || !g_up_low || !g_down_low)
        return std::unexpected(Error{error_category::invalid_result, "numerical analytics shift failed"});
    const double gamma_high = (*g_up - 2.0 * *v_up + *g_down) / (h * h);
    const double gamma_low = (*g_up_low - 2.0 * *v_down + *g_down_low) / (h * h);
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
        return std::unexpected(Error{error_category::invalid_result, "time shifts are unavailable at the boundary"});
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
    const double charm = (((*d_after - *d_after_low) - (*d_before - *d_before_low)) /
                          (2.0 * h * day_scale));
    const auto c_before = detail::shifted_value(engine, option, context, spot + h, volatility, rate, before);
    const auto c_before_mid = detail::shifted_value(engine, option, context, spot, volatility, rate, before);
    const auto c_before_low = detail::shifted_value(engine, option, context, spot - h, volatility, rate, before);
    const auto c_after = detail::shifted_value(engine, option, context, spot + h, volatility, rate, after);
    const auto c_after_mid = detail::shifted_value(engine, option, context, spot, volatility, rate, after);
    const auto c_after_low = detail::shifted_value(engine, option, context, spot - h, volatility, rate, after);
    if (!c_before || !c_before_mid || !c_before_low || !c_after || !c_after_mid || !c_after_low)
        return std::unexpected(Error{error_category::invalid_result, "time shift failed"});
    const double color = (((*c_after - 2.0 * *c_after_mid + *c_after_low) -
                           (*c_before - 2.0 * *c_before_mid + *c_before_low)) /
                          (h * h * day_scale));

    PricingResult output{{risk_measure::price, *p0}, {risk_measure::delta, delta},
                         {risk_measure::gamma, gamma}, {risk_measure::speed, speed},
                         {risk_measure::theta, theta}, {risk_measure::charm, charm},
                         {risk_measure::color, color}, {risk_measure::vega, vega},
                         {risk_measure::vanna, vanna}, {risk_measure::zomma, zomma},
                         {risk_measure::rho, rho}};
    for (const auto& value : output.values)
        if (value && !std::isfinite(*value))
            return std::unexpected(Error{error_category::invalid_result, "numerical analytics are non-finite"});
    return output;
}

template <typename Engine, typename Option>
[[nodiscard]] result<PricingResult> numerical_price(const Engine& engine, const Option& option,
                                                     const PricingContext& context,
                                                     NumericalShiftSettings settings = {})
{ return numerical_analytics(engine, option, context, settings); }

template <typename Engine, typename Option>
[[nodiscard]] result<ScenarioGridResult> scenario_grid(const Engine& engine, const Option& option,
                                                        const PricingContext& context,
                                                        std::span<const double> spots,
                                                        NumericalShiftSettings settings = {})
{
    ScenarioGridResult result;
    result.values.reserve(spots.size()); result.deltas.reserve(spots.size()); result.gammas.reserve(spots.size());
    for (double spot : spots) {
        auto shifted = detail::shifted_context(context, spot, context.parameters().volatility(),
                                               context.parameters().risk_free_rate(), context.valuation_time());
        if (!shifted) return std::unexpected(shifted.error());
        auto analytics = numerical_analytics(engine, option, *shifted, settings);
        if (!analytics) return std::unexpected(analytics.error());
        result.values.push_back(*analytics->get(risk_measure::price));
        result.deltas.push_back(*analytics->get(risk_measure::delta));
        result.gammas.push_back(*analytics->get(risk_measure::gamma));
    }
    return result;
}

template <typename Engine, typename Option>
[[nodiscard]] result<ScenarioGridResult> scenario_grid(const Engine& engine, const Option& option,
                                                        const PricingContext& context,
                                                        const std::vector<double>& spots,
                                                        NumericalShiftSettings settings = {})
{ return scenario_grid(engine, option, context, std::span<const double>{spots}, settings); }

template <typename Engine, typename Option>
[[nodiscard]] result<double> implied_volatility(const Engine& engine, const Option& option,
                                                const PricingContext& context, double observed_price,
                                                ImpliedVolatilitySettings settings = {})
{
    if (!std::isfinite(observed_price))
        return std::unexpected(Error{error_category::invalid_parameter, "observed price must be finite"});
    if (!std::isfinite(settings.lower_bound) || !std::isfinite(settings.upper_bound) ||
        settings.lower_bound <= 0.0 || settings.lower_bound >= settings.upper_bound ||
        !std::isfinite(settings.tolerance) || settings.tolerance <= 0.0 || settings.max_iterations <= 0)
        return std::unexpected(Error{error_category::invalid_parameter, "implied-volatility settings are invalid"});
    const auto evaluate = [&](double volatility) -> result<double> {
        auto shifted = detail::shifted_context(context, context.asset_price().value(), volatility,
                                               context.parameters().risk_free_rate(), context.valuation_time());
        if (!shifted) return std::unexpected(shifted.error());
        return detail::numerical_value(engine, option, *shifted);
    };
    double lo = settings.lower_bound, hi = settings.upper_bound;
    auto flo = evaluate(lo); auto fhi = evaluate(hi);
    if (!flo) return std::unexpected(flo.error()); if (!fhi) return std::unexpected(fhi.error());
    double elo = *flo - observed_price, ehi = *fhi - observed_price;
    if (std::abs(elo) <= settings.tolerance) return lo; if (std::abs(ehi) <= settings.tolerance) return hi;
    if ((elo < 0.0) == (ehi < 0.0))
        return std::unexpected(Error{error_category::unbracketed_volatility, "price is not bracketed by volatility bounds"});
    for (int i = 0; i < settings.max_iterations; ++i) {
        const double mid = std::midpoint(lo, hi); auto fm = evaluate(mid);
        if (!fm) return std::unexpected(fm.error());
        const double em = *fm - observed_price;
        if (std::abs(em) <= settings.tolerance || hi - lo <= settings.tolerance) return mid;
        if ((elo < 0.0) == (em < 0.0)) { lo = mid; elo = em; } else { hi = mid; ehi = em; }
    }
    return std::unexpected(Error{error_category::solver_non_convergence, "implied-volatility solver did not converge"});
}

template <typename Engine, typename Option>
[[nodiscard]] result<double> implied_coupon(const Engine& engine, const Option& option,
                                            const PricingContext& context, double observed_price,
                                            ImpliedCouponSettings settings = {})
requires requires(const Option& value, double coupon) { value.with_coupon_rate(coupon); }
{
    if (!std::isfinite(observed_price) || !std::isfinite(settings.lower_bound) ||
        !std::isfinite(settings.upper_bound) || settings.lower_bound < 0.0 ||
        settings.lower_bound >= settings.upper_bound || !std::isfinite(settings.tolerance) ||
        settings.tolerance <= 0.0 || settings.max_iterations <= 0)
        return std::unexpected(Error{error_category::invalid_parameter, "implied-coupon settings are invalid"});
    const auto evaluate = [&](double coupon) -> result<double> {
        auto replaced = option.with_coupon_rate(coupon);
        if (!replaced) return std::unexpected(replaced.error());
        auto priced = engine.price(*replaced, context);
        if (!priced) return std::unexpected(priced.error());
        const auto value = priced->get(risk_measure::price);
        if (!value || !std::isfinite(*value))
            return std::unexpected(Error{error_category::solver_non_finite, "implied-coupon pricing became non-finite"});
        return *value - observed_price;
    };
    double lo = settings.lower_bound, hi = settings.upper_bound;
    auto flo = evaluate(lo); auto fhi = evaluate(hi);
    if (!flo) return std::unexpected(flo.error()); if (!fhi) return std::unexpected(fhi.error());
    if (std::abs(*flo) <= settings.tolerance) return lo; if (std::abs(*fhi) <= settings.tolerance) return hi;
    if ((*flo < 0.0) == (*fhi < 0.0))
        return std::unexpected(Error{error_category::unbracketed_coupon, "price is not bracketed by coupon bounds"});
    for (int i = 0; i < settings.max_iterations; ++i) {
        const double mid = std::midpoint(lo, hi); auto fm = evaluate(mid);
        if (!fm) return std::unexpected(fm.error());
        if (std::abs(*fm) <= settings.tolerance || hi - lo <= settings.tolerance) return mid;
        if ((*flo < 0.0) == (*fm < 0.0)) { lo = mid; flo = fm; } else { hi = mid; fhi = fm; }
    }
    return std::unexpected(Error{error_category::solver_non_convergence, "implied-coupon solver did not converge"});
}

template <typename Option>
[[nodiscard]] auto replace_coupon(const Option& option, double coupon)
requires requires(const Option& value) { value.with_coupon_rate(coupon); }
{ return option.with_coupon_rate(coupon); }

} // namespace kiyosi
