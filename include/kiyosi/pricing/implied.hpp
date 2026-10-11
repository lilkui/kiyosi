#pragma once

#include <algorithm>
#include <cmath>
#include <limits>
#include <numeric>
#include <random>
#include <type_traits>
#include <utility>

#include <kiyosi/instruments/asian.hpp>
#include <kiyosi/instruments/accumulator.hpp>
#include <kiyosi/instruments/autocallable/phoenix.hpp>
#include <kiyosi/instruments/autocallable/snowball.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/detail/autocallable_traits.hpp>
#include <kiyosi/pricing/engines/autocallable/finite_difference.hpp>
#include <kiyosi/pricing/engines/autocallable/monte_carlo.hpp>
#include <kiyosi/pricing/engines/barrier/analytic.hpp>
#include <kiyosi/pricing/engines/binary_barrier/analytic.hpp>
#include <kiyosi/pricing/engines/vanilla/binomial.hpp>
#include <kiyosi/pricing/numerical_greeks.hpp>
#include <kiyosi/pricing/result.hpp>
#include <kiyosi/pricing/settings/implied.hpp>

namespace kiyosi {

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
/// Monte Carlo trials share one seed per solve when the engine has no explicit seed; tolerance
/// applies to that sampled price curve and does not bound sampling error.
/// CPU European Monte Carlo trials reuse per-solve samples with storage linear in path count.
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
    if (!std::isfinite(observed_price))
        return std::unexpected(Error{ErrorCategory::invalid_parameter, "observed price must be finite"});
    if (!std::isfinite(settings.lower_bound) || !std::isfinite(settings.upper_bound) ||
        settings.lower_bound <= 0.0 || settings.lower_bound >= settings.upper_bound ||
        !std::isfinite(settings.price_tolerance) || settings.price_tolerance <= 0.0 || !std::isfinite(settings.parameter_tolerance) ||
        settings.parameter_tolerance <= 0.0 || settings.max_iterations <= 0)
        return std::unexpected(Error{ErrorCategory::invalid_parameter,
                                     "implied-volatility settings are invalid"});
    if constexpr (requires { engine.settings().seed; }) {
        auto simulation = engine.settings();
        if (!simulation.seed) {
            simulation.seed = std::random_device{}();
            return implied_volatility(Engine{simulation}, option, context, observed_price, settings);
        }
    }

    if constexpr (std::same_as<Engine, CoxRossRubinsteinVanillaEngine>) {
        const auto step = detail::binomial_time_step(context, option.effective_date(), option.expiry_date(), engine.settings());
        if (!step) return std::unexpected(step.error());
        if (*step > 0.0) {
            const auto& parameters = context.model_parameters();
            const double drift = (parameters.risk_free_rate() - parameters.dividend_yield()) * *step;
            // Round into the valid tree domain so the endpoint probability stays in [0, 1].
            const double minimum = std::nextafter(std::abs(drift) / std::sqrt(*step),
                                                  std::numeric_limits<double>::infinity());
            if (std::isfinite(minimum)) settings.lower_bound = std::max(settings.lower_bound, minimum);
            if (settings.lower_bound >= settings.upper_bound)
                return std::unexpected(Error{ErrorCategory::unbracketed_volatility,
                                             "volatility bounds exclude the binomial tree domain"});
        }
    }

    if constexpr (std::same_as<Engine, MonteCarloAutocallableEngine<Option>>) {
        const auto valid = detail::validate_monte_carlo_autocallable(option, context, engine.settings());
        if (!valid) return std::unexpected(valid.error());
        const auto program = detail::autocallable_program(option);
        const auto initial = detail::autocallable_initial_state(option, context, program);
        if (!initial.settlement && detail::autocallable_fixed_value(option, context, program, initial))
            return std::unexpected(Error{ErrorCategory::unsupported_operation,
                                         "volatility does not affect the remaining cashflows"});
    }

    const auto evaluate = [&](double volatility) -> Result<double> {
        auto shifted = detail::shifted_context(context, context.spot_price(), volatility,
                                               context.model_parameters().risk_free_rate(),
                                               context.valuation_time());
        if (!shifted) return std::unexpected(shifted.error());
        auto priced = detail::numerical_value(engine, option, *shifted);
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
    bool identifiable = context.valuation_time() != start_of_day(option.expiry_date()); // NOLINT(misc-const-correctness): later state checks depend on the option type.
    if constexpr (std::same_as<Option, ArithmeticAveragePriceOption>)
        identifiable = identifiable && detail::arithmetic_average_adjusted_strike(option, context.valuation_time()) > 0.0;
    if constexpr (std::same_as<Option, BinarySnowballOption> || std::same_as<Option, TernarySnowballOption> ||
                  std::same_as<Option, SnowballOption> || std::same_as<Option, PhoenixOption>) {
        const auto program = detail::autocallable_program(option);
        const auto initial = detail::autocallable_initial_state(option, context, program);
        identifiable = identifiable && !initial.settlement &&
                       !detail::autocallable_fixed_value(option, context, program, initial);
    }
    if constexpr (requires { option.accumulated_quantity(); option.daily_quantity(); }) {
        const bool knocked_out_now = context.valuation_time() == start_of_day(context.valuation_date()) &&
                                     context.calendar().is_trading_day(context.valuation_date()) &&
                                     context.spot_price() >= option.knock_out_level();
        const bool accrues = detail::accumulator_accrues(option);
        // With zero rates and no new accrual, stopping an accumulated forward preserves its expectation.
        const bool exposed_quantity = option.accumulated_quantity() != 0.0 &&
                                      (context.model_parameters().risk_free_rate() != 0.0 ||
                                       context.model_parameters().dividend_yield() != 0.0);
        identifiable = identifiable && !knocked_out_now && (accrues || exposed_quantity);
    }
    if constexpr (requires { option.barrier_terms(); }) {
        const auto& terms = option.barrier_terms();
        const bool touched = *terms.was_touched_before(context.valuation_time()) ||
                             (terms.is_monitored_at(context.valuation_time()) &&
                              terms.is_breached_by(context.spot_price()));
        const bool monitoring_finished = !terms.is_continuous() &&
                                         start_of_day(terms.observation_dates().back()) <= context.valuation_time();
        if constexpr (requires { option.is_one_touch(); })
            identifiable = identifiable && !touched && !monitoring_finished;
        else {
            identifiable = identifiable && !(touched && !terms.is_knock_in()) &&
                           !(!touched && monitoring_finished && terms.is_knock_in());
            const bool monitors_expiry = terms.is_continuous() ||
                                         terms.observation_dates().back() == option.expiry_date();
            const bool payoff_excluded = !terms.is_knock_in() && monitors_expiry &&
                                         (terms.is_up() ? option.option_type() == OptionType::call && option.strike() >= terms.barrier_level()
                                                        : option.option_type() == OptionType::put && option.strike() <= terms.barrier_level());
            const bool has_rebate = [&] {
                if constexpr (requires { option.rebate(); }) return option.rebate() != 0.0;
                else return false;
            }();
            identifiable = identifiable && (!payoff_excluded || has_rebate);
        }
    }
    if (!identifiable)
        return std::unexpected(Error{ErrorCategory::unsupported_operation,
                                     "volatility does not affect the remaining cashflows"});
    if (std::abs(elo) <= settings.price_tolerance) return lo;
    if (std::abs(ehi) <= settings.price_tolerance) return hi;
    if ((elo < 0.0) == (ehi < 0.0))
        return std::unexpected(Error{ErrorCategory::unbracketed_volatility,
                                     "price is not bracketed by volatility bounds"});
    return detail::brent_implied_volatility(settings, elo, ehi, evaluate);
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

namespace detail {

inline Result<double> shifted_maturity_coupon(
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

inline std::vector<double> shifted_coupon_rates(
    const std::vector<double>& coupon_rates, double coupon)
{
    auto result = coupon_rates;
    const double shift = coupon - result.front();
    for (double& rate : result)
        rate += shift;
    return result;
}

inline Result<SnowballOption> replace_coupon(
    const SnowballOption& option, double coupon, CouponQuoteConvention convention)
{
    const double shift = coupon - option.knock_out_coupon_rates().front();
    const auto maturity_coupon_rate = shifted_maturity_coupon(
        option.maturity_coupon_rate(), shift, convention);
    if (!maturity_coupon_rate) return std::unexpected(maturity_coupon_rate.error());
    return make_snowball_option({.knock_out_coupon_rates = shifted_coupon_rates(
                                     option.knock_out_coupon_rates(), coupon),
                                 .maturity_coupon_rate = *maturity_coupon_rate,
                                 .initial_spot = option.initial_spot(),
                                 .knock_in_level = option.knock_in_level(),
                                 .knock_out_levels = option.knock_out_levels(),
                                 .upper_strike = option.upper_strike(),
                                 .lower_strike = option.lower_strike(),
                                 .observation_dates = option.observation_dates(),
                                 .knock_in_observation_mode = option.knock_in_observation_mode(),
                                 .barrier_state = option.barrier_state(),
                                 .principal_ratio = option.principal_ratio(),
                                 .effective_date = option.effective_date(),
                                 .expiry_date = option.expiry_date()});
}

inline Result<BinarySnowballOption> replace_coupon(
    const BinarySnowballOption& option, double coupon, CouponQuoteConvention convention)
{
    const double shift = coupon - option.knock_out_coupon_rates().front();
    const auto maturity_coupon_rate = shifted_maturity_coupon(
        option.maturity_coupon_rate(), shift, convention);
    if (!maturity_coupon_rate) return std::unexpected(maturity_coupon_rate.error());
    return make_binary_snowball_option({.knock_out_coupon_rates = shifted_coupon_rates(option.knock_out_coupon_rates(), coupon),
                                        .maturity_coupon_rate = *maturity_coupon_rate,
                                        .knock_out_levels = option.knock_out_levels(),
                                        .observation_dates = option.observation_dates(),
                                        .barrier_state = option.barrier_state(),
                                        .principal_ratio = option.principal_ratio(),
                                        .effective_date = option.effective_date(),
                                        .expiry_date = option.expiry_date()});
}

inline Result<TernarySnowballOption> replace_coupon(
    const TernarySnowballOption& option, double coupon, CouponQuoteConvention convention)
{
    const double shift = coupon - option.knock_out_coupon_rates().front();
    const auto maturity_coupon_rate = shifted_maturity_coupon(
        option.maturity_coupon_rate(), shift, convention);
    if (!maturity_coupon_rate) return std::unexpected(maturity_coupon_rate.error());
    return make_ternary_snowball_option({.knock_out_coupon_rates = shifted_coupon_rates(option.knock_out_coupon_rates(), coupon),
                                         .maturity_coupon_rate = *maturity_coupon_rate,
                                         .minimum_coupon_rate = option.minimum_coupon_rate(),
                                         .knock_in_level = option.knock_in_level(),
                                         .knock_out_levels = option.knock_out_levels(),
                                         .observation_dates = option.observation_dates(),
                                         .knock_in_observation_mode = option.knock_in_observation_mode(),
                                         .barrier_state = option.barrier_state(),
                                         .principal_ratio = option.principal_ratio(),
                                         .effective_date = option.effective_date(),
                                         .expiry_date = option.expiry_date()});
}

inline Result<PhoenixOption> replace_coupon(const PhoenixOption& option, double coupon)
{
    return make_phoenix_option({.coupon_rate = coupon,
                                .initial_spot = option.initial_spot(),
                                .knock_in_level = option.knock_in_level(),
                                .knock_out_levels = option.knock_out_levels(),
                                .coupon_barrier_levels = option.coupon_barrier_levels(),
                                .upper_strike = option.upper_strike(),
                                .lower_strike = option.lower_strike(),
                                .observation_dates = option.observation_dates(),
                                .knock_in_observation_mode = option.knock_in_observation_mode(),
                                .barrier_state = option.barrier_state(),
                                .principal_ratio = option.principal_ratio(),
                                .effective_date = option.effective_date(),
                                .expiry_date = option.expiry_date()});
}

template <typename Engine, typename Option, typename ReplaceCoupon>
[[nodiscard]] Result<double> solve_implied_coupon(
    const Engine& engine, const Option& option, const PricingContext& context, double observed_price,
    ImpliedCouponSettings settings, const ReplaceCoupon& replace_coupon)
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
    if constexpr (requires { engine.settings().seed; }) {
        auto simulation = engine.settings();
        if (!simulation.seed) {
            simulation.seed = std::random_device{}();
            return solve_implied_coupon(Engine{simulation}, option, context, observed_price, settings, replace_coupon);
        }
    }

    const auto price_at_coupon = [&](double coupon) -> Result<double> {
        auto replaced = replace_coupon(option, coupon);
        if (!replaced) return std::unexpected(replaced.error());
        auto priced = engine.price(*replaced, context);
        if (!priced) return std::unexpected(priced.error());
        if (!std::isfinite(*priced))
            return std::unexpected(Error{ErrorCategory::solver_non_finite,
                                         "implied-coupon pricing became non-finite"});
        return *priced;
    };

    const double lo = settings.lower_bound;
    const double hi = settings.upper_bound;
    const auto endpoint_prices = [&]() -> Result<std::array<double, 2>> {
        if constexpr (std::same_as<Engine, MonteCarloAutocallableEngine<Option>>) {
            const auto lower = replace_coupon(option, lo);
            if (!lower) return std::unexpected(lower.error());
            const auto upper = replace_coupon(option, hi);
            if (!upper) return std::unexpected(upper.error());
            return monte_carlo_coupon_prices(*lower, *upper, context, engine.settings());
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
    if constexpr (std::is_same_v<Engine, MonteCarloAutocallableEngine<Option>> ||
                  std::is_same_v<Engine, FiniteDifferenceAutocallableEngine<Option>>) {
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
}

} // namespace detail

/// Solves a Snowball engine's price curve in its knock-out coupon.
/// @param engine Pricing engine used for each trial coupon.
/// @param option Snowball-family instrument to value.
/// @param context Market state used for every trial.
/// @param observed_price Finite market price to match.
/// @param convention Whether the maturity coupon shifts with the quoted coupon.
/// @param settings Finite coupon bounds and convergence controls.
/// Monte Carlo trials share one seed per solve when the engine has no explicit seed; tolerance
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
/// Monte Carlo trials share one seed per solve when the engine has no explicit seed; tolerance
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

} // namespace kiyosi
