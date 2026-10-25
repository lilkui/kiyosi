#include <kiyosi/pricing/engines/range_accrual/analytic.hpp>

#include <algorithm>
#include <cmath>
#include <limits>

#include "../../detail/calendar_dates.hpp"
#include "../../detail/math.hpp"
#include "../../detail/numerical_greeks.hpp"
#include "../../detail/quadrature.hpp"

namespace kiyosi {

Result<double> AnalyticRangeAccrualEngine::price(const RangeAccrualOption& option, const PricingContext& context) const
{
    using namespace detail;
    const auto valuation = context.valuation_time();
    if (valuation > start_of_day(option.expiry_date()))
        return std::unexpected(Error{ErrorCategory::invalid_time_range, "valuation is after range accrual expiry"});
    if (const auto valid = validate_trading_expiry(context.calendar(), option.expiry_date()); !valid)
        return std::unexpected(valid.error());
    const auto dates = trading_dates(context.calendar(), option.effective_date(), option.expiry_date());
    const auto first = std::ranges::lower_bound(dates, valuation, {}, [](Date date) { return start_of_day(date); });
    const auto elapsed = first - dates.begin();
    if ((elapsed != 0 && !option.realized_hit_count()) || option.realized_hit_count().value_or(0) > elapsed)
        return std::unexpected(Error{ErrorCategory::invalid_parameter, "realized hit count is missing or exceeds prior observations"});
    double hits = option.realized_hit_count().value_or(0);
    auto future = first;
    if (future != dates.end() && start_of_day(*future) == valuation) {
        hits += option.lower_bound() <= context.spot_price() && context.spot_price() <= option.upper_bound() ? 1.0 : 0.0;
        ++future;
    }
    if (option.annual_coupon_rate() == 0.0) return 0.0;
    const double tau = actual_365_fixed_year_fraction(option.effective_date(), option.expiry_date());
    double log_scale = std::log(option.notional()) + std::log(option.annual_coupon_rate()) + std::log(tau) - std::log(static_cast<double>(dates.size()));
    if (future != dates.end()) {
        if (const auto valid = validate_curve_horizon(context, option.expiry_date()); !valid)
            return std::unexpected(valid.error());
        log_scale += curve_log_factor(context.model_parameters().risk_free_curve(), valuation, 0.0,
                                      actual_365_fixed_year_fraction(valuation, option.expiry_date()));
    }
    // Anchor log prices at the initial spot so adjacent bounds remain distinct.
    const double lower = option.lower_bound() == 0.0 ? -std::numeric_limits<double>::infinity() : log_price_ratio(option.lower_bound(), context.spot_price());
    const double upper = log_price_ratio(option.upper_bound(), context.spot_price());
    quadrature::Sum value;
    value.add(scaled_exponential(hits, log_scale));
    for (; future != dates.end(); ++future) {
        const double time = actual_365_fixed_year_fraction(valuation, *future);
        const double volatility = context.model_parameters().volatility();
        const double drift = -curve_log_factor(context.model_parameters().risk_free_curve(), valuation, 0.0, time) +
                             curve_log_factor(context.model_parameters().dividend_curve(), valuation, 0.0, time) -
                             0.5 * volatility * volatility * time;
        const double root_time = std::sqrt(time);
        const double diffusion = volatility * root_time;
        if (!std::isfinite(drift) || !std::isfinite(diffusion))
            return std::unexpected(Error{ErrorCategory::invalid_result, "range accrual transition is not finite"});
        const double log_probability = quadrature::log_interval_mass(
            standardize_forward(lower - drift, volatility, root_time),
            standardize_forward(upper - drift, volatility, root_time));
        value.add(std::exp(log_scale + log_probability));
    }
    return checked_price(value.total());
}

Result<PricingResult> AnalyticRangeAccrualEngine::price_with_greeks(
    const RangeAccrualOption& option, const PricingContext& context, GreeksRequest greeks, NumericalShiftSettings settings) const
{
    return detail::price_with_greeks(*this, option, context, greeks, settings);
}

} // namespace kiyosi
