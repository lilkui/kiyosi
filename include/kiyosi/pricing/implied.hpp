#pragma once

#include <cmath>
#include <numeric>

#include <kiyosi/instruments/structured/phoenix.hpp>
#include <kiyosi/instruments/structured/snowball.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/numerical_greeks.hpp>
#include <kiyosi/pricing/result.hpp>
#include <kiyosi/pricing/settings/implied.hpp>

namespace kiyosi {

/// Bisects the engine's price curve in volatility; the caller's bounds must bracket the quote.
template <typename Engine, typename Option>
[[nodiscard]] result<double> implied_volatility(
    const Engine& engine, const Option& option, const PricingContext& context, double observed_price,
    ImpliedVolatilitySettings settings = {})
{
    if (!std::isfinite(observed_price))
        return std::unexpected(Error{error_category::invalid_parameter, "observed price must be finite"});
    if (!std::isfinite(settings.lower_bound) || !std::isfinite(settings.upper_bound) ||
        settings.lower_bound <= 0.0 || settings.lower_bound >= settings.upper_bound ||
        !std::isfinite(settings.tolerance) || settings.tolerance <= 0.0 || settings.max_iterations <= 0)
        return std::unexpected(Error{error_category::invalid_parameter,
                                     "implied-volatility settings are invalid"});

    const auto evaluate = [&](double volatility) -> result<double> {
        auto shifted = detail::shifted_context(context, context.asset_price(), volatility,
                                               context.parameters().risk_free_rate(),
                                               context.valuation_time());
        if (!shifted) return std::unexpected(shifted.error());
        return detail::numerical_value(engine, option, *shifted);
    };

    double lo = settings.lower_bound;
    double hi = settings.upper_bound;
    auto flo = evaluate(lo);
    if (!flo) return std::unexpected(flo.error());
    auto fhi = evaluate(hi);
    if (!fhi) return std::unexpected(fhi.error());
    double elo = *flo - observed_price;
    const double ehi = *fhi - observed_price;
    if (std::abs(elo) <= settings.tolerance) return lo;
    if (std::abs(ehi) <= settings.tolerance) return hi;
    if ((elo < 0.0) == (ehi < 0.0))
        return std::unexpected(Error{error_category::unbracketed_volatility,
                                     "price is not bracketed by volatility bounds"});
    for (int iteration = 0; iteration < settings.max_iterations; ++iteration) {
        const double mid = std::midpoint(lo, hi);
        auto fm = evaluate(mid);
        if (!fm) return std::unexpected(fm.error());
        const double em = *fm - observed_price;
        if (std::abs(em) <= settings.tolerance || hi - lo <= settings.tolerance) return mid;
        if ((elo < 0.0) == (em < 0.0)) {
            lo = mid;
            elo = em;
        } else {
            hi = mid;
        }
    }
    return std::unexpected(Error{error_category::solver_non_convergence,
                                 "implied-volatility solver did not converge"});
}

namespace detail {

inline result<double> shifted_maturity_coupon(
    double maturity_coupon, double shift, coupon_quote_convention convention)
{
    switch (convention) {
    case coupon_quote_convention::linked_maturity:
        return maturity_coupon + shift;
    case coupon_quote_convention::fixed_maturity:
        return maturity_coupon;
    }
    return std::unexpected(Error{error_category::invalid_parameter,
                                 "coupon quote convention is invalid"});
}

inline std::vector<double> shifted_coupon_rates(
    const std::vector<double>& coupon_rates, double coupon)
{
    auto result = coupon_rates;
    const double shift = coupon - result.front();
    for (double& rate : result)
        rate += shift;
    return result;
}

inline result<SnowballOption> replace_coupon(
    const SnowballOption& option, double coupon, coupon_quote_convention convention)
{
    const double shift = coupon - option.knock_out_coupon_rates().front();
    const auto maturity_coupon = shifted_maturity_coupon(
        option.maturity_coupon_rate(), shift, convention);
    if (!maturity_coupon) return std::unexpected(maturity_coupon.error());
    return make_snowball_option({.knock_out_coupon_rates = shifted_coupon_rates(
                                     option.knock_out_coupon_rates(), coupon),
                                 .maturity_coupon_rate = *maturity_coupon,
                                 .initial_price = option.initial_price(),
                                 .knock_in_price = option.knock_in_price(),
                                 .knock_out_prices = option.knock_out_prices(),
                                 .upper_strike = option.upper_strike(),
                                 .lower_strike = option.lower_strike(),
                                 .observation_dates = option.observation_dates(),
                                 .frequency = option.knock_in_frequency(),
                                 .touch_status = option.touch_status(),
                                 .principal_ratio = option.principal_ratio(),
                                 .effective = option.effective(),
                                 .expiry = option.expiry()});
}

inline result<BinarySnowballOption> replace_coupon(
    const BinarySnowballOption& option, double coupon, coupon_quote_convention convention)
{
    const double shift = coupon - option.knock_out_coupon_rates().front();
    const auto maturity_coupon = shifted_maturity_coupon(
        option.maturity_coupon_rate(), shift, convention);
    if (!maturity_coupon) return std::unexpected(maturity_coupon.error());
    return make_binary_snowball_option({
        .knock_out_coupon_rates = shifted_coupon_rates(option.knock_out_coupon_rates(), coupon),
        .maturity_coupon_rate = *maturity_coupon,
        .initial_price = option.initial_price(),
        .knock_out_prices = option.knock_out_prices(),
        .upper_strike = option.upper_strike(),
        .lower_strike = option.lower_strike(),
        .observation_dates = option.observation_dates(),
        .touch_status = option.touch_status(),
        .principal_ratio = option.principal_ratio(),
        .effective = option.effective(),
        .expiry = option.expiry()});
}

inline result<TernarySnowballOption> replace_coupon(
    const TernarySnowballOption& option, double coupon, coupon_quote_convention convention)
{
    const double shift = coupon - option.knock_out_coupon_rates().front();
    const auto maturity_coupon = shifted_maturity_coupon(
        option.maturity_coupon_rate(), shift, convention);
    if (!maturity_coupon) return std::unexpected(maturity_coupon.error());
    return make_ternary_snowball_option({
        .knock_out_coupon_rates = shifted_coupon_rates(option.knock_out_coupon_rates(), coupon),
        .maturity_coupon_rate = *maturity_coupon,
        .minimal_coupon_rate = option.minimal_coupon_rate(),
        .initial_price = option.initial_price(),
        .knock_in_price = option.knock_in_price(),
        .knock_out_prices = option.knock_out_prices(),
        .upper_strike = option.upper_strike(),
        .lower_strike = option.lower_strike(),
        .observation_dates = option.observation_dates(),
        .frequency = option.knock_in_frequency(),
        .touch_status = option.touch_status(),
        .principal_ratio = option.principal_ratio(),
        .effective = option.effective(),
        .expiry = option.expiry()});
}

inline result<PhoenixOption> replace_coupon(const PhoenixOption& option, double coupon)
{
    return make_phoenix_option({.coupon_rate = coupon,
                                .initial_price = option.initial_price(),
                                .knock_in_price = option.knock_in_price(),
                                .knock_out_prices = option.knock_out_prices(),
                                .coupon_barriers = option.coupon_barriers(),
                                .upper_strike = option.upper_strike(),
                                .lower_strike = option.lower_strike(),
                                .observation_dates = option.observation_dates(),
                                .frequency = option.knock_in_frequency(),
                                .touch_status = option.touch_status(),
                                .principal_ratio = option.principal_ratio(),
                                .effective = option.effective(),
                                .expiry = option.expiry()});
}

template <typename Engine, typename Option, typename ReplaceCoupon>
[[nodiscard]] result<double> solve_implied_coupon(
    const Engine& engine, const Option& option, const PricingContext& context, double observed_price,
    ImpliedCouponSettings settings, const ReplaceCoupon& replace_coupon)
{
    if (!std::isfinite(observed_price) || !std::isfinite(settings.lower_bound) ||
        !std::isfinite(settings.upper_bound) || settings.lower_bound < 0.0 ||
        settings.lower_bound >= settings.upper_bound || !std::isfinite(settings.tolerance) ||
        settings.tolerance <= 0.0 || settings.max_iterations <= 0)
        return std::unexpected(Error{error_category::invalid_parameter,
                                     "implied-coupon settings are invalid"});

    const auto evaluate = [&](double coupon) -> result<double> {
        auto replaced = replace_coupon(option, coupon);
        if (!replaced) return std::unexpected(replaced.error());
        auto priced = engine.price(*replaced, context);
        if (!priced) return std::unexpected(priced.error());
        const auto value = priced->get(risk_measure::price);
        if (!value) return std::unexpected(value.error());
        if (!*value || !std::isfinite(**value))
            return std::unexpected(Error{error_category::solver_non_finite,
                                         "implied-coupon pricing became non-finite"});
        return **value - observed_price;
    };

    double lo = settings.lower_bound;
    double hi = settings.upper_bound;
    auto flo = evaluate(lo);
    if (!flo) return std::unexpected(flo.error());
    auto fhi = evaluate(hi);
    if (!fhi) return std::unexpected(fhi.error());
    if (std::abs(*flo) <= settings.tolerance) return lo;
    if (std::abs(*fhi) <= settings.tolerance) return hi;
    if ((*flo < 0.0) == (*fhi < 0.0))
        return std::unexpected(Error{error_category::unbracketed_coupon,
                                     "price is not bracketed by coupon bounds"});
    for (int iteration = 0; iteration < settings.max_iterations; ++iteration) {
        const double mid = std::midpoint(lo, hi);
        auto fm = evaluate(mid);
        if (!fm) return std::unexpected(fm.error());
        if (std::abs(*fm) <= settings.tolerance || hi - lo <= settings.tolerance) return mid;
        if ((*flo < 0.0) == (*fm < 0.0)) {
            lo = mid;
            flo = fm;
        } else {
            hi = mid;
            fhi = fm;
        }
    }
    return std::unexpected(Error{error_category::solver_non_convergence,
                                 "implied-coupon solver did not converge"});
}

} // namespace detail

/// Bisects a Snowball engine's price curve in its knock-out coupon.
template <typename Engine, typename Option>
[[nodiscard]] result<double> implied_coupon(
    const Engine& engine, const Option& option, const PricingContext& context, double observed_price,
    coupon_quote_convention convention, ImpliedCouponSettings settings = {})
    requires requires(const Option& value, double coupon) {
        detail::replace_coupon(value, coupon, coupon_quote_convention::fixed_maturity);
    }
{
    return detail::solve_implied_coupon(
        engine, option, context, observed_price, settings,
        [convention](const Option& value, double coupon) {
            return detail::replace_coupon(value, coupon, convention);
        });
}

/// Bisects the engine's price curve in an unambiguous product coupon, such as a Phoenix coupon.
template <typename Engine, typename Option>
[[nodiscard]] result<double> implied_coupon(
    const Engine& engine, const Option& option, const PricingContext& context, double observed_price,
    ImpliedCouponSettings settings = {})
    requires requires(const Option& value, double coupon) { detail::replace_coupon(value, coupon); }
{
    return detail::solve_implied_coupon(
        engine, option, context, observed_price, settings,
        [](const Option& value, double coupon) { return detail::replace_coupon(value, coupon); });
}

} // namespace kiyosi
