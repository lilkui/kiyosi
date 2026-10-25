#include <kiyosi/pricing/engines/barrier/analytic.hpp>

#include "../../detail/numerical_greeks.hpp"
#include <kiyosi/pricing/implied.hpp>

#include <algorithm>
#include <cmath>

#include "../../detail/analytic_barrier.hpp"
#include "../../detail/barrier_settlement.hpp"
#include "../../detail/math.hpp"

namespace kiyosi {
using namespace detail;
namespace {

Result<double> price_barrier(
    const BarrierOption& option, const PricingContext& context, bool& schedule_validated)
{
    const auto valid = validate_valuation_within_instrument_life(context.valuation_time(), option.effective_date(), option.expiry_date());
    if (!valid) return std::unexpected(valid.error());
    if (!schedule_validated && option.observation_mode() == ObservationMode::scheduled) {
        auto schedule_valid = validate_observation_dates(option.observation_dates(), option.effective_date(),
                                                         option.expiry_date(), context.calendar());
        if (!schedule_valid) return std::unexpected(schedule_valid.error());
        schedule_validated = true;
        // shortcut: scheduled dates use a BGK barrier shift; use finite differences for exact monitoring dates.
    }
    if (const auto settled = resolved_barrier_price(option, context)) return *settled;
    const double t = actual_365_fixed_year_fraction(context.valuation_time(), option.expiry_date());
    const double spot = context.spot_price();
    const double rate = (*context.model_parameters().risk_free_rate());
    const double dividend = (*context.model_parameters().dividend_yield());
    const double sigma = context.model_parameters().volatility();
    const auto& terms = option.barrier_terms();
    const bool upper = terms.is_up();
    const bool knock_in = terms.is_knock_in();
    const auto shift = analytic_barrier_log_shift(terms, spot, sigma);
    if (!shift) return std::unexpected(shift.error());
    const bool strike_above_barrier = log_price_ratio(option.strike(), terms.barrier_level()) > *shift;
    const bool payoff_requires_touch = terminal_payoff_requires_barrier_touch(terms, option.option_type(), option.strike());
    const bool needs_vanilla = payoff_requires_touch ? knock_in : (knock_in ? upper == strike_above_barrier : upper != strike_above_barrier);
    double a = 0.0;
    if (t == 0.0 || needs_vanilla) {
        const auto vanilla = price_at_volatility(
            *make_european_option(option.option_type(), option.strike(), option.effective_date(), option.expiry_date()), context,
            sigma, GreeksRequest{});
        if (!vanilla) return std::unexpected(vanilla.error());
        a = vanilla->price();
    }
    if (t == 0.0) {
        return checked_price(knock_in ? option.rebate() : a);
    }
    const bool hit_rebate = option.rebate() != 0.0 && option.rebate_timing() == RebateTiming::at_hit;
    const double log_ratio = log_price_ratio(terms.barrier_level(), spot) + *shift;
    double hit_rebate_value = 0.0;
    if (hit_rebate) {
        const double drift = rate - dividend - 0.5 * sigma * sigma;
        const auto value = barrier_hit_value(option.rebate(), std::abs(log_ratio), upper,
                                             drift, sigma, t, rate);
        if (!value) return std::unexpected(value.error());
        hit_rebate_value = *value;
    }
    const double root_time = sigma * std::sqrt(t);
    const double x = option.strike();
    const double carry = rate - dividend;
    const double mu = carry / sigma / sigma - 0.5;
    const double log_moneyness = log_price_ratio(spot, x);
    const double x2 = std::isfinite(mu) ? -log_ratio / root_time + (1.0 + mu) * root_time
                                        : standardize_forward(-log_ratio + carry * t, sigma, std::sqrt(t)) + 0.5 * root_time;
    const double y1 = (2.0 * log_ratio + log_moneyness) / root_time + (1.0 + mu) * root_time;
    const double y2 = log_ratio / root_time + (1.0 + mu) * root_time;
    const bool call = option.option_type() == OptionType::call;
    const double eta = upper ? -1.0 : 1.0;
    const double phi = call ? 1.0 : -1.0;
    const auto reflected = [&](double amount, double discount, double argument, bool at_strike, bool asset) {
        const double exponent = 2.0 * (asset ? mu + 1.0 : mu) * log_ratio;
        return std::isfinite(exponent) ? scaled_normal_cdf(amount, discount + exponent, argument)
                                       : scaled_barrier_reflection_tail(amount, discount, log_ratio, at_strike ? log_price_ratio(terms.barrier_level(), x) + *shift : 0.0,
                                                                        carry, sigma, t, asset ? 0.5 : -0.5);
    };
    // Evaluate only the Reiner-Rubinstein terms needed by this contract.
    const auto b = [&] {
        return phi * scaled_normal_cdf(spot, -dividend * t, phi * x2) - phi * scaled_normal_cdf(x, -rate * t, phi * x2 - phi * root_time);
    };
    const auto c = [&] {
        return phi * reflected(spot, -dividend * t, eta * y1, true, true) -
               phi * reflected(x, -rate * t, eta * y1 - eta * root_time, true, false);
    };
    const auto d = [&] {
        return phi * reflected(spot, -dividend * t, eta * y2, false, true) -
               phi * reflected(x, -rate * t, eta * y2 - eta * root_time, false, false);
    };
    const auto e = [&] {
        return scaled_normal_cdf(option.rebate(), -rate * t, eta * x2 - eta * root_time) -
               reflected(option.rebate(), -rate * t, eta * y2 - eta * root_time, false, false);
    };
    const auto rebate = [&] {
        return option.rebate_timing() == RebateTiming::at_hit
                   ? hit_rebate_value
                   : scaled_normal_cdf(option.rebate(), -rate * t, -eta * x2 + eta * root_time) +
                         reflected(option.rebate(), -rate * t, eta * y2 - eta * root_time, false, false);
    };
    double value = 0.0;
    if (payoff_requires_touch) {
        value = knock_in ? a + e() : rebate();
    } else if (call) {
        if (knock_in) value = upper ? (strike_above_barrier ? a + e() : b() - c() + d() + e()) : (strike_above_barrier ? c() + e() : a - b() + d() + e());
        else value = upper ? (strike_above_barrier ? rebate() : a - b() + c() - d() + rebate()) : (strike_above_barrier ? a - c() + rebate() : b() - d() + rebate());
    } else {
        if (knock_in) value = upper ? (strike_above_barrier ? a - b() + d() + e() : c() + e()) : (strike_above_barrier ? b() - c() + d() + e() : a + e());
        else value = upper ? (strike_above_barrier ? b() - d() + rebate() : a - c() + rebate()) : (strike_above_barrier ? a - b() + c() - d() + rebate() : rebate());
    }
    if (!std::isfinite(value))
        return std::unexpected(Error{ErrorCategory::invalid_result, "analytic pricing produced a non-finite result"});
    return std::max(value, 0.0);
}

struct BarrierPricer {
    mutable bool schedule_validated{};
    Result<double> price(const BarrierOption& option, const PricingContext& context) const
    {
        return price_barrier(option, context, schedule_validated);
    }
};
} // namespace

template <>
Result<double> implied_volatility<AnalyticBarrierEngine, BarrierOption>(
    const AnalyticBarrierEngine&, const BarrierOption& option, const PricingContext& context,
    double observed_price, ImpliedVolatilitySettings settings)
{
    if (auto valid = detail::require_flat_rates(context); !valid) return std::unexpected(valid.error());
    return implied_volatility(BarrierPricer{}, option, context, observed_price, settings);
}

Result<double> AnalyticBarrierEngine::price(const BarrierOption& option, const PricingContext& context) const
{
    if (auto valid = detail::require_flat_rates(context); !valid) return std::unexpected(valid.error());
    return BarrierPricer{}.price(option, context);
}

Result<PricingResult> AnalyticBarrierEngine::price_with_greeks(const BarrierOption& option, const PricingContext& context,
                                                               GreeksRequest greeks, NumericalShiftSettings settings) const
{
    if (auto valid = detail::require_flat_rates(context); !valid) return std::unexpected(valid.error());
    return detail::price_with_greeks(BarrierPricer{}, option, context, greeks, settings);
}

} // namespace kiyosi
