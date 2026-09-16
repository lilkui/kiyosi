#pragma once

#include <cmath>
#include <numeric>

#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/detail/revaluation.hpp>
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

/// Bisects the engine's price curve in the product coupon; requires a coupon-bearing option.
template <typename Engine, typename Option>
[[nodiscard]] result<double> implied_coupon(
    const Engine& engine, const Option& option, const PricingContext& context, double observed_price,
    ImpliedCouponSettings settings = {})
requires requires(const Option& value, double coupon) { value.with_coupon_rate(coupon); }
{
    if (!std::isfinite(observed_price) || !std::isfinite(settings.lower_bound) ||
        !std::isfinite(settings.upper_bound) || settings.lower_bound < 0.0 ||
        settings.lower_bound >= settings.upper_bound || !std::isfinite(settings.tolerance) ||
        settings.tolerance <= 0.0 || settings.max_iterations <= 0)
        return std::unexpected(Error{error_category::invalid_parameter,
                                     "implied-coupon settings are invalid"});

    const auto evaluate = [&](double coupon) -> result<double> {
        auto replaced = option.with_coupon_rate(coupon);
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

} // namespace kiyosi
