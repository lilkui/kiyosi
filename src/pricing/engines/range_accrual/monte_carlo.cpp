#include <kiyosi/pricing/engines/range_accrual/monte_carlo.hpp>

#include <algorithm>
#include <array>
#include <cmath>
#include <limits>
#include <vector>

#include "../../detail/calendar_dates.hpp"
#include "../../detail/math.hpp"
#include "../../detail/numerical_greeks.hpp"
#include "../monte_carlo_host.hpp"
#include "../monte_carlo_range_accrual.hpp"

namespace kiyosi {

Result<double> MonteCarloRangeAccrualEngine::price(const RangeAccrualOption& option, const PricingContext& context) const
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
    const RangeAccrualMonteCarloParameters parameters{
        0.0, option.lower_bound() == 0.0 ? -std::numeric_limits<double>::infinity() : log_price_ratio(option.lower_bound(), context.spot_price()), log_price_ratio(option.upper_bound(), context.spot_price()), hits, {std::exp(log_scale), log_scale}};
    if (future == dates.end()) return checked_price(parameters.payment.apply(hits));

    std::vector<SimulationStep> steps;
    steps.reserve(static_cast<std::size_t>(dates.end() - future));
    double previous = 0.0;
    for (; future != dates.end(); ++future) {
        const double time = actual_365_fixed_year_fraction(valuation, *future);
        const double dt = time - previous;
        const double volatility = context.model_parameters().volatility();
        const double drift = -curve_log_factor(context.model_parameters().risk_free_curve(), valuation, previous, time) +
                             curve_log_factor(context.model_parameters().dividend_curve(), valuation, previous, time) -
                             0.5 * volatility * volatility * dt;
        const double diffusion = volatility * std::sqrt(dt);
        if (!std::isfinite(drift) || !std::isfinite(diffusion))
            return std::unexpected(Error{ErrorCategory::invalid_result, "range accrual transition is not finite"});
        steps.push_back({drift, diffusion, {}});
        previous = time;
    }
    // The final increment is conditioned out; each earlier increment drives the next observation.
    const auto dimension = steps.size() - 1;
    if (const auto valid = validate_monte_carlo_dimension(settings_.sampling, dimension); !valid)
        return std::unexpected(valid.error());
    if (settings_.backend == MonteCarloBackend::cuda) {
#if KIYOSI_HAS_CUDA
        const auto value = cuda_mean(cuda_range_accrual_price(
            {settings_.path_count, settings_.seed ? *settings_.seed : random_seed(), parameters, settings_.antithetic}, steps));
        if (!value) return std::unexpected(value.error());
        return checked_price(*value);
#else
        return std::unexpected(Error{ErrorCategory::backend_unavailable, "CUDA support is not enabled in this build"});
#endif
    }
    const auto prices = cpu_path_means<1>(settings_, [&](auto& generator) -> Result<std::array<double, 1>> {
        std::normal_distribution<double> normal;
        double log_spot = parameters.log_spot;
        double accrued = parameters.payment.apply(parameters.initial_hits);
        for (std::size_t index = 0; index < steps.size(); ++index) {
            const auto& step = steps[index];
            accrued += range_accrual_value(log_spot, step, parameters);
            if (index + 1 != steps.size()) {
                log_spot += step.drift + step.diffusion * draw_monte_carlo_normal(generator, normal);
                if (!std::isfinite(log_spot))
                    return std::unexpected(Error{ErrorCategory::invalid_result, "range accrual log price is not finite"});
            }
        }
        return std::array{accrued}; }, dimension);
    if (!prices) return std::unexpected(prices.error());
    return checked_price((*prices)[0]);
}

Result<PricingResult> MonteCarloRangeAccrualEngine::price_with_greeks(
    const RangeAccrualOption& option, const PricingContext& context, GreeksRequest greeks, NumericalShiftSettings settings) const
{
    return detail::price_with_greeks(*this, option, context, greeks, settings);
}

} // namespace kiyosi
