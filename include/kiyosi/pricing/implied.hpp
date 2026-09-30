#pragma once

#include <cmath>
#include <numeric>
#include <random>

#include <kiyosi/instruments/structured/phoenix.hpp>
#include <kiyosi/instruments/structured/snowball.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/numerical_greeks.hpp>
#include <kiyosi/pricing/result.hpp>
#include <kiyosi/pricing/settings/implied.hpp>

namespace kiyosi {

/// Bisects the engine's price curve in volatility; the caller's bounds must bracket the quote.
/// @tparam Engine Pricing engine providing `price(option, context)`.
/// @tparam Option Instrument accepted by the engine.
/// @param engine Pricing engine used for each trial volatility.
/// @param option Instrument to value.
/// @param context Market state whose volatility is replaced for each trial.
/// @param observed_price Finite market price to match.
/// @param settings Positive bounds and convergence controls.
/// Monte Carlo trials share one seed per solve when the engine has no explicit seed; tolerance
/// applies to that sampled price curve and does not bound sampling error.
/// A root is not guaranteed unique; expiry payoffs and terminated knock-out contracts
/// independent of volatility, including fixed touch payments, are rejected.
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
        !std::isfinite(settings.tolerance) || settings.tolerance <= 0.0 || settings.max_iterations <= 0)
        return std::unexpected(Error{ErrorCategory::invalid_parameter,
                                     "implied-volatility settings are invalid"});
    if constexpr (requires { engine.settings().seed; }) {
        auto simulation = engine.settings();
        if (!simulation.seed) {
            simulation.seed = std::random_device{}();
            return implied_volatility(Engine{simulation}, option, context, observed_price, settings);
        }
    }

    const auto evaluate = [&](double volatility) -> Result<double> {
        auto shifted = detail::shifted_context(context, context.spot_price(), volatility,
                                               context.model_parameters().risk_free_rate(),
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
    // Pricing above validates the contract and history before these state checks.
    bool identifiable = context.valuation_time() != start_of_day(option.expiry_date());
    if constexpr (requires { option.barrier_state(); }) {
        identifiable = identifiable && option.barrier_state() != AutocallableBarrierState::knocked_out;
        for (std::size_t i = 0; i < option.observation_dates().size(); ++i)
            if (context.valuation_time() == start_of_day(option.observation_dates()[i]) &&
                context.spot_price() >= option.knock_out_levels()[i])
                identifiable = false;
        if constexpr (std::same_as<Option, BinarySnowballOption>)
            identifiable = identifiable && start_of_day(option.observation_dates().back()) >= context.valuation_time();
    }
    if constexpr (requires { option.barrier_terms(); }) {
        const auto& terms = option.barrier_terms();
        const bool touched = *terms.was_touched_before(context.valuation_time()) ||
                             (terms.is_monitored_at(context.valuation_time()) &&
                              terms.is_breached_by(context.spot_price()));
        const bool monitoring_finished = !terms.is_continuous() &&
                                        !terms.has_remaining_observation(context.valuation_time());
        if constexpr (requires { option.is_one_touch(); })
            identifiable = identifiable && !touched && !monitoring_finished;
        else
            identifiable = identifiable && !(touched && !terms.is_knock_in()) &&
                           !(!touched && monitoring_finished && terms.is_knock_in());
    }
    if (!identifiable)
        return std::unexpected(Error{ErrorCategory::unsupported_operation,
                                     "volatility does not affect the remaining cashflows"});
    if (std::abs(elo) <= settings.tolerance) return lo;
    if (std::abs(ehi) <= settings.tolerance) return hi;
    if ((elo < 0.0) == (ehi < 0.0))
        return std::unexpected(Error{ErrorCategory::unbracketed_volatility,
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
    return std::unexpected(Error{ErrorCategory::solver_non_convergence,
                                 "implied-volatility solver did not converge"});
}

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
    ImpliedCouponSettings settings, bool shifts_maturity_coupon, const ReplaceCoupon& replace_coupon)
{
    if (!std::isfinite(observed_price) || !std::isfinite(settings.lower_bound) ||
        !std::isfinite(settings.upper_bound) ||
        settings.lower_bound >= settings.upper_bound || !std::isfinite(settings.tolerance) ||
        settings.tolerance <= 0.0 || settings.max_iterations <= 0)
        return std::unexpected(Error{ErrorCategory::invalid_parameter,
                                     "implied-coupon settings are invalid"});
    if constexpr (requires { engine.settings().seed; }) {
        auto simulation = engine.settings();
        if (!simulation.seed) {
            simulation.seed = std::random_device{}();
            return solve_implied_coupon(Engine{simulation}, option, context, observed_price, settings, shifts_maturity_coupon, replace_coupon);
        }
    }

    const auto evaluate = [&](double coupon) -> Result<double> {
        auto replaced = replace_coupon(option, coupon);
        if (!replaced) return std::unexpected(replaced.error());
        auto priced = engine.price(*replaced, context);
        if (!priced) return std::unexpected(priced.error());
        if (!std::isfinite(*priced))
            return std::unexpected(Error{ErrorCategory::solver_non_finite,
                                         "implied-coupon pricing became non-finite"});
        return *priced - observed_price;
    };

    double lo = settings.lower_bound;
    double hi = settings.upper_bound;
    auto flo = evaluate(lo);
    if (!flo) return std::unexpected(flo.error());
    auto fhi = evaluate(hi);
    if (!fhi) return std::unexpected(fhi.error());
    // A coupon needs an unsettled accrual period or a maturity coupon that moves.
    bool remaining_coupon = false;
    for (std::size_t i = 0; i < option.observation_dates().size(); ++i) {
        const auto date = option.observation_dates()[i];
        const auto period_start = [&] {
            if constexpr (requires { option.coupon_barrier_levels(); })
                if (i > 0) return option.observation_dates()[i - 1];
            return option.effective_date();
        }();
        if (start_of_day(date) >= context.valuation_time() && date > period_start)
            remaining_coupon = true;
    }
    if constexpr (requires { option.maturity_coupon_rate(); }) {
        bool maturity_coupon_payable = true;
        if constexpr (requires { option.knock_in_level(); })
            maturity_coupon_payable = option.barrier_state() != AutocallableBarrierState::knocked_in;
        remaining_coupon = remaining_coupon || (shifts_maturity_coupon && maturity_coupon_payable);
    }
    if (option.barrier_state() == AutocallableBarrierState::knocked_out || !remaining_coupon)
        return std::unexpected(Error{ErrorCategory::unsupported_operation,
                                     "coupon does not affect the remaining cashflows"});
    if (std::abs(*flo) <= settings.tolerance) return lo;
    if (std::abs(*fhi) <= settings.tolerance) return hi;
    if ((*flo < 0.0) == (*fhi < 0.0))
        return std::unexpected(Error{ErrorCategory::unbracketed_coupon,
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
    return std::unexpected(Error{ErrorCategory::solver_non_convergence,
                                 "implied-coupon solver did not converge"});
}

} // namespace detail

/// Bisects a Snowball engine's price curve in its knock-out coupon.
/// @param engine Pricing engine used for each trial coupon.
/// @param option Snowball-family instrument to value.
/// @param context Market state used for every trial.
/// @param observed_price Finite market price to match.
/// @param convention Whether the maturity coupon shifts with the quoted coupon.
/// @param settings Finite coupon bounds and convergence controls.
/// Monte Carlo trials share one seed per solve when the engine has no explicit seed; tolerance
/// applies to that sampled price curve and does not bound sampling error.
/// A root is not guaranteed unique; states with no remaining quoted coupon exposure are rejected.
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
        engine, option, context, observed_price, settings,
        convention == CouponQuoteConvention::shift_maturity_coupon, [convention](const Option& value, double coupon) {
            return detail::replace_coupon(value, coupon, convention);
        });
}

/// Bisects the engine's price curve in an unambiguous product coupon, such as a Phoenix coupon.
/// Monte Carlo trials share one seed per solve when the engine has no explicit seed; tolerance
/// applies to that sampled price curve and does not bound sampling error.
/// @return Implied coupon, or a validation, bracketing, pricing, or convergence error.
template <typename Engine, typename Option>
[[nodiscard]] Result<double> implied_coupon(
    const Engine& engine, const Option& option, const PricingContext& context, double observed_price,
    ImpliedCouponSettings settings = {})
    requires requires(const Option& value, double coupon) { detail::replace_coupon(value, coupon); }
{
    return detail::solve_implied_coupon(
        engine, option, context, observed_price, settings, false,
        [](const Option& value, double coupon) { return detail::replace_coupon(value, coupon); });
}

} // namespace kiyosi
