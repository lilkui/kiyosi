#include <kiyosi/pricing/engines/asian/monte_carlo.hpp>

#include <array>
#include <cmath>
#include <random>
#include <type_traits>
#include <vector>

#include <kiyosi/pricing/engines/asian/analytic.hpp>

#include "../../detail/math.hpp"
#include "../../detail/numerical_greeks.hpp"
#include "../../detail/rate_term_structure.hpp"
#include "../monte_carlo_asian.hpp"
#include "../monte_carlo_host.hpp"

namespace kiyosi {
namespace {

double log_average_forward(const PricingContext& context, Timestamp expiry, double lead, double future)
{
    const auto& model = context.model_parameters();
    if (model.risk_free_curve().is_flat() && model.dividend_curve().is_flat()) {
        const double carry = (*model.risk_free_rate()) - (*model.dividend_yield());
        const double x = carry * future;
        return carry * lead + (x == 0.0 ? 0.0 : x > 0.0 ? x + std::log(-std::expm1(-x) / x)
                                                        : std::log(std::expm1(x) / x));
    }
    auto times = detail::curve_times(context, expiry);
    times.insert(times.end(), {lead, lead + future});
    std::ranges::sort(times);
    times.erase(std::ranges::unique(times).begin(), times.end());
    double log_mean = -HUGE_VAL;
    for (std::size_t index = 1; index < times.size(); ++index) {
        const double start = times[index - 1], end = times[index];
        if (start < lead) continue;
        const double initial = -detail::curve_log_factor(model.risk_free_curve(), context.valuation_time(), 0.0, start) +
                               detail::curve_log_factor(model.dividend_curve(), context.valuation_time(), 0.0, start);
        const double x = -detail::curve_log_factor(model.risk_free_curve(), context.valuation_time(), start, end) +
                         detail::curve_log_factor(model.dividend_curve(), context.valuation_time(), start, end);
        const double log_growth = x == 0.0 ? 0.0 : x > 0.0 ? x + std::log(-std::expm1(-x) / x)
                                                           : std::log(std::expm1(x) / x);
        log_mean = detail::asian_log_add(log_mean, initial + log_growth + std::log((end - start) / future));
    }
    return log_mean;
}

template <typename Option>
Result<PricingResult> price_asian(const Option& option, const PricingContext& context,
                                  MonteCarloSettings settings, GreeksRequest greeks)
{
    const auto time = detail::time_to_expiry(context.valuation_time(), option.effective_date(), option.expiry_date());
    if (!time) return std::unexpected(time.error());
    const auto valuation = context.valuation_time();
    const auto start = start_of_day(option.averaging_start_date());
    const double realized = option.realized_average();
    if ((valuation > start && realized == 0.0) || (valuation <= start && realized != 0.0))
        return std::unexpected(Error{ErrorCategory::invalid_parameter, "realized average must match the elapsed averaging period"});
    const bool call = option.option_type() == OptionType::call;
    const double sign = call ? 1.0 : -1.0;
    const double spot = context.spot_price(), strike = option.strike();
    if (*time == 0.0)
        return make_pricing_result(std::max(sign * ((realized > 0.0 ? realized : spot) - strike), 0.0));

    const auto& model = context.model_parameters();
    const bool flat = model.risk_free_curve().is_flat() && model.dividend_curve().is_flat();
    if constexpr (std::same_as<Option, GeometricAveragePriceOption>) {
        if (!flat)
            return std::unexpected(Error{ErrorCategory::unsupported_operation, "geometric Asian Monte Carlo requires flat rates"});
    }
    const auto expiry = start_of_day(option.expiry_date());
    if (const auto coverage = detail::validate_curve_horizon(context, expiry); !coverage)
        return std::unexpected(coverage.error());
    const double sigma = model.volatility();
    const double carry = flat ? (*model.risk_free_rate()) - (*model.dividend_yield()) : 0.0;
    const double log_discount = detail::curve_log_factor(model.risk_free_curve(), valuation, 0.0, *time);
    const bool single = option.averaging_start_date() == option.expiry_date();
    const double lead = valuation < start && !single ? detail::actual_365_fixed_year_fraction(valuation, start) : 0.0; // NOLINT(readability-suspicious-call-argument): valuation precedes averaging.
    const double future = single ? *time : detail::actual_365_fixed_year_fraction(std::max(valuation, start), option.expiry_date());
    const double period = single ? future : detail::actual_365_fixed_year_fraction(start, option.expiry_date());
    const double weight = future / period;
    const bool delta = greeks.has(Greek::delta), gamma = greeks.has(Greek::gamma);
    if constexpr (std::same_as<Option, ArithmeticAveragePriceOption>) {
        if (!single && detail::arithmetic_average_adjusted_strike(option, valuation) <= 0.0) {
            const double log_mean = log_average_forward(context, expiry, lead, future);
            const double first = call ? std::exp(log_discount + std::log(weight) + log_mean) : 0.0;
            const double elapsed_weight = detail::actual_365_fixed_year_fraction(start, valuation) / period;
            const double accrued = std::max(std::fma(elapsed_weight, realized, -strike), 0.0);
            const auto price = flat ? TurnbullWakemanArithmeticAveragePriceEngine{}.price(option, context)
                                    : detail::checked_price(call ? detail::scaled_exponential(spot, log_discount + std::log(weight) + log_mean) +
                                                                       detail::scaled_exponential(accrued, log_discount)
                                                                 : 0.0);
            if (!price) return std::unexpected(price.error());
            return make_pricing_result(*price, {{Greek::delta, delta ? std::optional{first} : std::nullopt},
                                                {Greek::gamma, gamma ? std::optional{0.0} : std::nullopt}});
        }
    }

    detail::AsianMonteCarloParameters parameters;
    parameters.spot = spot;
    parameters.strike = strike;
    parameters.realized = realized;
    parameters.weight = weight;
    parameters.log_discount = log_discount;
    parameters.call = call;
    parameters.geometric = single || std::same_as<Option, GeometricAveragePriceOption>;
    parameters.step_count = settings.step_count;
    parameters.lead = lead > 0.0;
    const double drift_rate = carry - 0.5 * sigma * sigma;
    std::vector<double> interval_drifts;
    if (parameters.geometric) {
        const double variance_time = single ? *time : lead + future / 3.0;
        parameters.drift = single && !flat
                               ? -detail::curve_log_factor(model.risk_free_curve(), valuation, 0.0, *time) +
                                     detail::curve_log_factor(model.dividend_curve(), valuation, 0.0, *time) - 0.5 * sigma * sigma * *time
                               : weight * drift_rate * (single ? *time : lead + future / 2.0);
        parameters.log_base_width = std::log(sigma) + 0.5 * std::log(variance_time);
        parameters.log_width = parameters.log_base_width + std::log(weight);
        parameters.width = sigma * (weight * std::sqrt(variance_time));
        parameters.initial_moneyness = weight * detail::log_price_ratio(spot, strike);
        if (valuation > start) parameters.initial_moneyness += (1.0 - weight) * detail::log_price_ratio(realized, strike);
        parameters.standardized_moneyness = detail::standardize_forward(
            parameters.initial_moneyness / weight + parameters.drift / weight, sigma, std::sqrt(variance_time));
    } else {
        const double interval = future / settings.step_count;
        parameters.drift = drift_rate * interval;
        parameters.width = sigma * std::sqrt(interval);
        parameters.log_width = std::log(sigma) + 0.5 * std::log(interval);
        parameters.lead_drift = drift_rate * lead;
        parameters.lead_width = sigma * std::sqrt(lead);
        parameters.lead_width_ratio = std::sqrt(lead / interval);
        parameters.interval_forward = carry * interval;
        if (!flat) {
            const auto carry_between = [&](double from, double to) {
                return -detail::curve_log_factor(model.risk_free_curve(), valuation, from, to) +
                       detail::curve_log_factor(model.dividend_curve(), valuation, from, to);
            };
            parameters.interval_forward = carry_between(lead, lead + interval);
            parameters.drift = parameters.interval_forward - 0.5 * sigma * sigma * interval;
            parameters.lead_drift = carry_between(0.0, lead) - 0.5 * sigma * sigma * lead;
            interval_drifts.reserve(static_cast<std::size_t>(settings.step_count - 1));
            for (int step = 2; step <= settings.step_count; ++step) {
                const double from = lead + future * (step - 1) / settings.step_count;
                const double to = step == settings.step_count ? *time : lead + future * step / settings.step_count;
                const double drift = carry_between(from, to) - 0.5 * sigma * sigma * interval;
                if (!std::isfinite(drift))
                    return std::unexpected(Error{ErrorCategory::invalid_result, "Asian Monte Carlo interval drift is non-finite"});
                interval_drifts.push_back(drift);
            }
        }
    }
    if (!std::isfinite(parameters.drift) || !std::isfinite(parameters.width) ||
        !std::isfinite(parameters.log_discount) || !std::isfinite(parameters.lead_drift) ||
        !std::isfinite(parameters.lead_width) || !std::isfinite(parameters.interval_forward))
        return std::unexpected(Error{ErrorCategory::invalid_result, "Asian Monte Carlo parameters are non-finite"});

    std::array<double, 3> means{};
    if (settings.backend == MonteCarloBackend::cuda) {
#if KIYOSI_HAS_CUDA
        const auto sampled = detail::cuda_asian_prices({settings.path_count,
                                                        settings.seed ? *settings.seed : detail::random_seed(), parameters,
                                                        settings.antithetic, delta, gamma},
                                                       nullptr, interval_drifts);
        const auto valid = detail::cuda_mean(sampled.result);
        if (!valid) return std::unexpected(valid.error());
        means = {sampled.payoff_means[0], sampled.payoff_means[1], sampled.payoff_means[2]};
#else
        return std::unexpected(Error{ErrorCategory::backend_unavailable, "CUDA support is not enabled in this build"});
#endif
    } else {
        const auto dimensions = parameters.geometric ? 1 : settings.step_count - 1 + static_cast<int>(parameters.lead);
        const auto values = detail::cpu_path_means<3>(settings, [&](auto& generator) -> Result<std::array<double, 3>> {
            std::normal_distribution<double> normal;
            const auto sampled = parameters.sample([&] { return detail::draw_monte_carlo_normal(generator, normal); }, delta, gamma,
                                                   interval_drifts.empty() ? nullptr : interval_drifts.data());
            const std::array output{sampled.price, sampled.delta, sampled.gamma};
            for (const double value : output)
                if (!std::isfinite(value))
                    return std::unexpected(Error{ErrorCategory::invalid_result, "Asian Monte Carlo produced a non-finite sample"});
            return output; }, static_cast<std::size_t>(dimensions));
        if (!values) return std::unexpected(values.error());
        means = *values;
    }
    return make_pricing_result(means[0], {{Greek::delta, delta ? std::optional{means[1]} : std::nullopt},
                                          {Greek::gamma, gamma ? std::optional{means[2]} : std::nullopt}});
}
} // namespace

Result<double> MonteCarloAsianEngine::price(const ArithmeticAveragePriceOption& option, const PricingContext& context) const
{
    return detail::price_value(price_asian(option, context, settings_, {}));
}
Result<double> MonteCarloAsianEngine::price(const GeometricAveragePriceOption& option, const PricingContext& context) const
{
    return detail::price_value(price_asian(option, context, settings_, {}));
}
Result<PricingResult> MonteCarloAsianEngine::price_with_greeks(
    const ArithmeticAveragePriceOption& option, const PricingContext& context, GreeksRequest greeks, NumericalShiftSettings settings) const
{
    return detail::evaluate_greeks(*this, option, context, greeks, settings, [&](const auto& engine) {
        return price_asian(option, context, engine.settings(), greeks);
    });
}
Result<PricingResult> MonteCarloAsianEngine::price_with_greeks(
    const GeometricAveragePriceOption& option, const PricingContext& context, GreeksRequest greeks, NumericalShiftSettings settings) const
{
    return detail::evaluate_greeks(*this, option, context, greeks, settings, [&](const auto& engine) {
        return price_asian(option, context, engine.settings(), greeks);
    });
}
} // namespace kiyosi
