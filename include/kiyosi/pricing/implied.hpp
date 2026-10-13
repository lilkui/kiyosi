#pragma once

#include <algorithm>
#include <array>
#include <cmath>
#include <concepts>
#include <cstddef>
#include <numeric>
#include <utility>

#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/numerical_greeks.hpp>
#include <kiyosi/pricing/result.hpp>
#include <kiyosi/pricing/settings/implied.hpp>

namespace kiyosi {

class AnalyticBarrierEngine;
class AnalyticBinaryBarrierEngine;
class CoxRossRubinsteinVanillaEngine;
class FiniteDifferenceAutocallableEngine;
class MonteCarloAccumulatorEngine;
class MonteCarloAutocallableEngine;
class MonteCarloVanillaEngine;

namespace detail {

// Brent interpolation keeps the bracket and falls back to bisection when progress is uncertain.
template <typename Evaluate>
Result<double> brent_implied_volatility(const ImpliedVolatilitySettings& settings,
                                        double fa, double fb, const Evaluate& evaluate)
{
    double a = settings.lower_bound;
    double b = settings.upper_bound;
    if (std::abs(fa) < std::abs(fb)) {
        std::swap(a, b);
        std::swap(fa, fb);
    }
    double c = a, fc = fa, d = c;
    bool bisected = true;
    for (int iteration = 0; iteration < settings.max_iterations; ++iteration) {
        const double scale = std::max({std::abs(fa), std::abs(fb), std::abs(fc)});
        const double ya = fa / scale, yb = fb / scale, yc = fc / scale;
        double trial = std::lerp(a, b, ya / (ya - yb));
        if (fa != fc && fb != fc)
            trial = a * (yb / (ya - yb)) * (yc / (ya - yc)) +
                    b * (ya / (yb - ya)) * (yc / (yb - yc)) +
                    c * (ya / (yc - ya)) * (yb / (yc - yb));
        const double quarter = std::lerp(a, b, 0.25);
        if (!std::isfinite(trial) || trial <= std::min(quarter, b) || trial >= std::max(quarter, b) ||
            (bisected && std::abs(trial - b) >= 0.5 * std::abs(b - c)) ||
            (!bisected && std::abs(trial - b) >= 0.5 * std::abs(c - d)) ||
            (bisected && std::abs(b - c) <= settings.parameter_tolerance) ||
            (!bisected && std::abs(c - d) <= settings.parameter_tolerance)) {
            trial = std::midpoint(a, b);
            bisected = true;
        } else {
            bisected = false;
        }
        const auto residual = evaluate(trial);
        if (!residual) return std::unexpected(residual.error());
        if (std::abs(*residual) <= settings.price_tolerance || std::abs(b - a) <= settings.parameter_tolerance)
            return trial;
        d = c;
        c = b;
        fc = fb;
        if ((fa < 0.0) != (*residual < 0.0)) {
            b = trial;
            fb = *residual;
        } else {
            a = trial;
            fa = *residual;
        }
        if (std::abs(fa) < std::abs(fb)) {
            std::swap(a, b);
            std::swap(fa, fb);
        }
    }
    return std::unexpected(Error{ErrorCategory::solver_non_convergence,
                                 "implied-volatility solver did not converge"});
}

// Callers validate settings, endpoint residuals, and identifiability before iterating.
template <typename Settings, typename Evaluate>
Result<double> bisect_implied(const Settings& settings, double lower_residual,
                              const Evaluate& evaluate, const char* failure_message)
{
    double lo = settings.lower_bound;
    double hi = settings.upper_bound;
    for (int iteration = 0; iteration < settings.max_iterations; ++iteration) {
        const double mid = std::midpoint(lo, hi);
        auto residual = evaluate(mid);
        if (!residual) return std::unexpected(residual.error());
        if (std::abs(*residual) <= settings.price_tolerance || hi - lo <= settings.parameter_tolerance) return mid;
        if ((lower_residual < 0.0) == (*residual < 0.0)) {
            lo = mid;
            lower_residual = *residual;
        } else {
            hi = mid;
        }
    }
    return std::unexpected(Error{ErrorCategory::solver_non_convergence, failure_message});
}

template <typename Engine, typename Option>
Result<ImpliedVolatilitySettings> prepare_implied_volatility(
    const Engine&, const Option&, const PricingContext&, ImpliedVolatilitySettings settings) { return settings; }

template <typename Option>
bool volatility_affects_price(const Option&, const PricingContext&) { return true; }

// Keep validation and seeding identical when a solver supplies request-local pricing storage.
template <typename Engine, typename Option, typename Price, typename Prepare = std::nullptr_t>
[[nodiscard]] Result<double> solve_implied_volatility(
    const Engine& engine, const Option& option, const PricingContext& context, double observed_price,
    ImpliedVolatilitySettings settings, const Price& price, const Prepare& prepare = nullptr)
{
    if (!std::isfinite(observed_price))
        return std::unexpected(Error{ErrorCategory::invalid_parameter, "observed price must be finite"});
    if (!std::isfinite(settings.lower_bound) || !std::isfinite(settings.upper_bound) ||
        settings.lower_bound <= 0.0 || settings.lower_bound >= settings.upper_bound ||
        !std::isfinite(settings.price_tolerance) || settings.price_tolerance <= 0.0 || !std::isfinite(settings.parameter_tolerance) ||
        settings.parameter_tolerance <= 0.0 || settings.max_iterations <= 0)
        return std::unexpected(Error{ErrorCategory::invalid_parameter,
                                     "implied-volatility settings are invalid"});
    return with_monte_carlo_seed<Engine, Option>(engine, [&](const Engine& seeded) -> Result<double> {
        const auto prepared = [&] {
            if constexpr (std::same_as<Prepare, std::nullptr_t>) return prepare_implied_volatility(seeded, option, context, settings);
            else return prepare(seeded, settings);
        }();
        if (!prepared) return std::unexpected(prepared.error());
        settings = *prepared;

        const auto evaluate = [&](double volatility) -> Result<double> {
            auto shifted = detail::shifted_context(context, context.spot_price(), volatility,
                                                   context.model_parameters().risk_free_rate(),
                                                   context.valuation_time());
            if (!shifted) return std::unexpected(shifted.error());
            auto priced = price(seeded, option, *shifted);
            if (!priced) return std::unexpected(priced.error());
            return *priced - observed_price;
        };

        const double lo = settings.lower_bound;
        const double hi = settings.upper_bound;
        auto flo = evaluate(lo);
        if (!flo) return std::unexpected(flo.error());
        auto fhi = evaluate(hi);
        if (!fhi) return std::unexpected(fhi.error());
        const double elo = *flo;
        const double ehi = *fhi;
        // Pricing above validates the contract and history before these state checks.
        if (context.valuation_time() == start_of_day(option.expiry_date()) || !volatility_affects_price(option, context))
            return std::unexpected(Error{ErrorCategory::unsupported_operation,
                                         "volatility does not affect the remaining cashflows"});
        if (std::abs(elo) <= settings.price_tolerance) return lo;
        if (std::abs(ehi) <= settings.price_tolerance) return hi;
        if ((elo < 0.0) == (ehi < 0.0))
            return std::unexpected(Error{ErrorCategory::unbracketed_volatility,
                                         "price is not bracketed by volatility bounds"});
        return detail::brent_implied_volatility(settings, elo, ehi, evaluate);
    });
}

} // namespace detail

/// Uses safeguarded Brent interpolation in volatility; the caller's bounds must bracket the quote.
/// For CRR trees, the interval is intersected with the tree's admissible volatility range.
/// @tparam Engine Pricing engine providing `price(option, context)`.
/// @tparam Option Instrument accepted by the engine.
/// @param engine Pricing engine used for each trial volatility.
/// @param option Instrument to value.
/// @param context Market state whose volatility is replaced for each trial.
/// @param observed_price Finite market price to match.
/// @param settings Positive bounds and convergence controls.
/// Built-in Monte Carlo trials share one seed per solve when the engine has no explicit seed; tolerance
/// applies to that sampled price curve and does not bound sampling error.
/// CPU European Monte Carlo trials reuse per-solve samples with storage linear in path count.
/// CUDA accumulator and autocallable trials reuse a workspace local to each solve.
/// A root is not guaranteed unique; known states with volatility-independent remaining
/// cashflows, including fixed touch payments, are rejected.
/// Built-in autocallable Monte Carlo engines reject fixed cashflows after input validation,
/// before executing a simulation backend.
/// @return Implied volatility, or a validation, bracketing, pricing, or convergence error.
template <typename Engine, typename Option>
[[nodiscard]] Result<double> implied_volatility(
    const Engine& engine, const Option& option, const PricingContext& context, double observed_price,
    ImpliedVolatilitySettings settings = {})
{
    return detail::solve_implied_volatility(engine, option, context, observed_price, settings,
                                            [](const auto& pricing_engine, const auto& instrument, const auto& shifted) {
                                                return detail::numerical_value(pricing_engine, instrument, shifted);
                                            });
}

/// Scheduled analytic barriers validate their calendar once per solve.
template <>
[[nodiscard]] KIYOSI_EXPORT Result<double> implied_volatility<AnalyticBarrierEngine, BarrierOption>(
    const AnalyticBarrierEngine& engine, const BarrierOption& option, const PricingContext& context,
    double observed_price, ImpliedVolatilitySettings settings);

template <>
[[nodiscard]] KIYOSI_EXPORT Result<double> implied_volatility<AnalyticBinaryBarrierEngine, BinaryBarrierOption>(
    const AnalyticBinaryBarrierEngine& engine, const BinaryBarrierOption& option, const PricingContext& context,
    double observed_price, ImpliedVolatilitySettings settings);

template <>
[[nodiscard]] KIYOSI_EXPORT Result<double> implied_volatility<AnalyticBinaryBarrierEngine, TouchOption>(
    const AnalyticBinaryBarrierEngine& engine, const TouchOption& option, const PricingContext& context,
    double observed_price, ImpliedVolatilitySettings settings);

template <>
[[nodiscard]] KIYOSI_EXPORT Result<double> implied_volatility<MonteCarloVanillaEngine, EuropeanOption>(
    const MonteCarloVanillaEngine& engine, const EuropeanOption& option, const PricingContext& context,
    double observed_price, ImpliedVolatilitySettings settings);

/// Reuses request-local exercise buffers across trials; growth factors are recomputed.
template <>
[[nodiscard]] KIYOSI_EXPORT Result<double> implied_volatility<MonteCarloVanillaEngine, AmericanOption>(
    const MonteCarloVanillaEngine& engine, const AmericanOption& option, const PricingContext& context,
    double observed_price, ImpliedVolatilitySettings settings);

/// Finite-difference autocallables validate immutable contracts and calendars once per solve.
template <AutocallableProduct Note>
[[nodiscard]] KIYOSI_EXPORT Result<double> implied_volatility(
    const FiniteDifferenceAutocallableEngine& engine, const Note& option, const PricingContext& context,
    double observed_price, ImpliedVolatilitySettings settings = {});

/// Reuses CUDA buffers within one solve without sharing storage between requests.
template <>
[[nodiscard]] KIYOSI_EXPORT Result<double> implied_volatility<MonteCarloAccumulatorEngine, Accumulator>(
    const MonteCarloAccumulatorEngine& engine, const Accumulator& option, const PricingContext& context,
    double observed_price, ImpliedVolatilitySettings settings);

/// Reuses CUDA buffers within one solve without sharing storage between requests.
template <AutocallableProduct Note>
[[nodiscard]] KIYOSI_EXPORT Result<double> implied_volatility(
    const MonteCarloAutocallableEngine& engine, const Note& option, const PricingContext& context,
    double observed_price, ImpliedVolatilitySettings settings = {});

namespace detail {

template <typename Engine, typename Option, typename ReplaceCoupon, typename Price>
[[nodiscard]] Result<double> solve_implied_coupon(
    const Engine& engine, const Option& option, const PricingContext& context, double observed_price,
    ImpliedCouponSettings settings, const ReplaceCoupon& replace_coupon, const Price& price)
{
    if (!std::isfinite(observed_price) || !std::isfinite(settings.lower_bound) ||
        !std::isfinite(settings.upper_bound) ||
        settings.lower_bound >= settings.upper_bound || !std::isfinite(settings.price_tolerance) ||
        settings.price_tolerance <= 0.0 || !std::isfinite(settings.parameter_tolerance) ||
        settings.parameter_tolerance <= 0.0 || settings.max_iterations <= 0)
        return std::unexpected(Error{ErrorCategory::invalid_parameter,
                                     "implied-coupon settings are invalid"});
    const auto valid = validate_autocallable_note(option);
    if (!valid) return std::unexpected(valid.error());
    return with_monte_carlo_seed<Engine, Option>(engine, [&](const Engine& seeded) -> Result<double> {
        const auto price_at_coupon = [&](double coupon) -> Result<double> {
            auto replaced = replace_coupon(option, coupon);
            if (!replaced) return std::unexpected(replaced.error());
            auto priced = price(seeded, *replaced, context);
            if (!priced) return std::unexpected(priced.error());
            if (!std::isfinite(*priced))
                return std::unexpected(Error{ErrorCategory::solver_non_finite,
                                             "implied-coupon pricing became non-finite"});
            return *priced;
        };

        const double lo = settings.lower_bound;
        const double hi = settings.upper_bound;
        const auto endpoint_prices = [&]() -> Result<std::array<double, 2>> {
            if constexpr (requires { coupon_endpoint_prices(seeded, option, option, context); }) {
                const auto lower = replace_coupon(option, lo);
                if (!lower) return std::unexpected(lower.error());
                const auto upper = replace_coupon(option, hi);
                if (!upper) return std::unexpected(upper.error());
                return coupon_endpoint_prices(seeded, *lower, *upper, context);
            } else {
                const auto lower = price_at_coupon(lo);
                if (!lower) return std::unexpected(lower.error());
                const auto upper = price_at_coupon(hi);
                if (!upper) return std::unexpected(upper.error());
                return std::array{*lower, *upper};
            }
        }();
        if (!endpoint_prices) return std::unexpected(endpoint_prices.error());
        const double lower_price = (*endpoint_prices)[0];
        const double upper_price = (*endpoint_prices)[1];
        // Coupon shifts change payoffs affinely; equal endpoint prices have no sampled quote exposure.
        if (lower_price == upper_price)
            return std::unexpected(Error{ErrorCategory::unsupported_operation,
                                         "coupon does not affect the remaining cashflows"});
        const double flo = lower_price - observed_price;
        const double fhi = upper_price - observed_price;
        if (std::abs(flo) <= settings.price_tolerance) return lo;
        if (std::abs(fhi) <= settings.price_tolerance) return hi;
        if ((flo < 0.0) == (fhi < 0.0))
            return std::unexpected(Error{ErrorCategory::unbracketed_coupon,
                                         "price is not bracketed by coupon bounds"});
        const auto evaluate = [&](double coupon) -> Result<double> {
            auto priced = price_at_coupon(coupon);
            if (!priced) return std::unexpected(priced.error());
            return *priced - observed_price;
        };
        if constexpr (has_affine_coupon_price<Engine, Option>) {
            // Coupon payoffs are affine; halve prices only if their span overflows.
            const double span = upper_price - lower_price;
            const double fraction = std::isfinite(span)
                                        ? (observed_price - lower_price) / span
                                        : (0.5 * observed_price - 0.5 * lower_price) / (0.5 * upper_price - 0.5 * lower_price);
            const double coupon = std::lerp(lo, hi, fraction);
            const auto residual = evaluate(coupon);
            if (!residual) return std::unexpected(residual.error());
            if (std::abs(*residual) <= settings.price_tolerance) return coupon;
            // FD spline limiting or simulation roundoff can require bisection.
        }
        return detail::bisect_implied(settings, flo, evaluate,
                                      "implied-coupon solver did not converge");
    });
}

template <typename Engine, typename Option, typename ReplaceCoupon>
[[nodiscard]] Result<double> solve_implied_coupon(
    const Engine& engine, const Option& option, const PricingContext& context, double observed_price,
    ImpliedCouponSettings settings, const ReplaceCoupon& replace_coupon)
{
    return solve_implied_coupon(engine, option, context, observed_price, settings, replace_coupon,
                                [](const auto& pricing_engine, const auto& instrument, const auto& market) {
                                    return pricing_engine.price(instrument, market);
                                });
}

} // namespace detail

/// Solves a Snowball engine's price curve in its knock-out coupon.
/// @param engine Pricing engine used for each trial coupon.
/// @param option Snowball-family instrument to value.
/// @param context Market state used for every trial.
/// @param observed_price Finite market price to match.
/// @param convention Whether the maturity coupon shifts with the quoted coupon.
/// @param settings Finite coupon bounds and convergence controls.
/// Built-in Monte Carlo trials share one seed per solve when the engine has no explicit seed; tolerance
/// applies to that sampled price curve and does not bound sampling error.
/// Built-in Monte Carlo engines share paths between coupon bounds. Built-in Monte Carlo and FD
/// engines try interpolation, verify its price residual, then bisect if needed.
/// States with no quoted coupon exposure on the engine's sampled price curve are rejected.
/// @return Implied coupon, or a validation, bracketing, pricing, or convergence error.
template <typename Engine, typename Option>
[[nodiscard]] Result<double> implied_coupon(
    const Engine& engine, const Option& option, const PricingContext& context, double observed_price,
    CouponQuoteConvention convention, ImpliedCouponSettings settings = {})
    requires requires(const Option& value, double coupon) {
        detail::replace_coupon(value, coupon, CouponQuoteConvention::preserve_maturity_coupon);
    }
{
    return detail::solve_implied_coupon(
        engine, option, context, observed_price, settings, [convention](const Option& value, double coupon) {
            return detail::replace_coupon(value, coupon, convention);
        });
}

/// Solves the engine's price curve in an unambiguous product coupon, such as a Phoenix coupon.
/// Built-in Monte Carlo trials share one seed per solve when the engine has no explicit seed; tolerance
/// applies to that sampled price curve and does not bound sampling error.
/// Built-in Monte Carlo engines share paths between coupon bounds. Built-in Monte Carlo and FD
/// engines try interpolation, verify its price residual, then bisect if needed.
/// States with no quoted coupon exposure on the engine's sampled price curve are rejected.
/// @return Implied coupon, or a validation, bracketing, pricing, or convergence error.
template <typename Engine, typename Option>
[[nodiscard]] Result<double> implied_coupon(
    const Engine& engine, const Option& option, const PricingContext& context, double observed_price,
    ImpliedCouponSettings settings = {})
    requires requires(const Option& value, double coupon) { detail::replace_coupon(value, coupon); }
{
    return detail::solve_implied_coupon(
        engine, option, context, observed_price, settings,
        [](const Option& value, double coupon) { return detail::replace_coupon(value, coupon); });
}

/// Reuse grids, buffers and compatible factors within one solve; requests share no storage.
template <AutocallableProduct Note>
[[nodiscard]] KIYOSI_EXPORT Result<double> implied_coupon(
    const FiniteDifferenceAutocallableEngine& engine, const Note& option, const PricingContext& context,
    double observed_price, CouponQuoteConvention convention, ImpliedCouponSettings settings = {})
    requires requires(const Note& value, double coupon) {
        detail::replace_coupon(value, coupon, CouponQuoteConvention::preserve_maturity_coupon);
    };

[[nodiscard]] KIYOSI_EXPORT Result<double> implied_coupon(
    const FiniteDifferenceAutocallableEngine& engine, const PhoenixOption& option, const PricingContext& context,
    double observed_price, ImpliedCouponSettings settings = {});

} // namespace kiyosi
