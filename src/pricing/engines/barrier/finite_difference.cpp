#include <kiyosi/pricing/engines/barrier/finite_difference.hpp>

#include <algorithm>
#include <cmath>
#include <vector>

#include "../../detail/black_scholes.hpp"
#include "../../detail/fd_grid.hpp"
#include "../../detail/fd_scheme.hpp"
#include "../../detail/math.hpp"

namespace kiyosi {
using namespace detail;
namespace {
result<double> knockout_fd(const BarrierOption& option, const PricingContext& context, FiniteDifferenceSettings settings)
{
    auto valid = validate_life(context.valuation_time(), option.effective(), option.expiry());
    if (!valid) return std::unexpected(valid.error());
    if (option.observation() == observation_mode::scheduled) {
        auto schedule = validate_schedule(option.schedule(), option.effective(), option.expiry(), context.calendar());
        if (!schedule) return std::unexpected(schedule.error());
    }
    const double maturity = actual_365(context.valuation_time(), option.expiry());
    const double spot = context.asset_price(), strike = option.strike();
    if (maturity == 0.0) return std::max((option.type() == option_type::call ? spot - strike : strike - spot), 0.0);
    const double rate = context.parameters().risk_free_rate(), dividend = context.parameters().dividend_yield(), volatility = context.parameters().volatility();
    const double barrier = option.barrier();
    const int asset_steps = settings.asset_steps;
    const auto space = make_spatial_grid(settings, std::max(4.0 * strike, 4.0 * spot), {spot, strike, barrier});
    if (!space) return std::unexpected(space.error());
    const double upper = space->upper, spacing = space->spacing;
    const int time_steps = settings.time_steps;
    const double theta = scheme_theta(settings.scheme);
    const bool upper_barrier = option.barrier_terms().is_up();
    std::vector<double> observation_times;
    if (option.observation() == observation_mode::scheduled) {
        for (auto value : option.observation_dates()) {
            if (value < context.valuation_time()) continue;
            const double event_time = actual_365(context.valuation_time(), value);
            if (event_time >= 0.0 && event_time <= maturity) observation_times.push_back(event_time);
        }
    }
    const auto grid = finite_difference_grid(maturity, time_steps, observation_times);
    if (auto stable = check_explicit_stability(settings.scheme, grid, volatility, rate, asset_steps);
        !stable)
        return std::unexpected(stable.error());
    auto active = [&](double time) { return option.observation() == observation_mode::continuous || std::binary_search(observation_times.begin(), observation_times.end(), time); };
    std::sort(observation_times.begin(), observation_times.end());
    auto payoff = [&](double asset) { return std::max((option.type() == option_type::call ? asset - strike : strike - asset), 0.0); };
    auto knocked = [&](double asset) { return upper_barrier ? asset >= barrier : asset <= barrier; };
    auto rebate_value = [&](double tau) { return option.rebate_payment() == rebate_timing::at_hit ? option.rebate() : option.rebate() * std::exp(-rate * tau); };
    std::vector<double> old(space->size());
    for (int index = 0; index <= asset_steps; ++index) old[index] = payoff(spacing * index);
    if (active(maturity)) for (int index = 0; index <= asset_steps; ++index) if (knocked(spacing * index)) old[index] = option.rebate();
    const auto boundary = [&](double tau) {
        Boundaries edges{option.type() == option_type::put ? strike * std::exp(-rate * tau) : 0.0,
                         option.type() == option_type::call ? upper * std::exp(-dividend * tau) - strike * std::exp(-rate * tau) : 0.0};
        if (option.observation() == observation_mode::continuous) {
            if (knocked(0.0)) edges.lower = rebate_value(tau);
            if (knocked(upper)) edges.upper = rebate_value(tau);
        }
        return edges;
    };
    const auto constraint = [&](int index, double tau) -> std::optional<double> {
        if (option.observation() == observation_mode::continuous && knocked(spacing * index))
            return rebate_value(tau);
        return std::nullopt;
    };
    const auto marched = march_backward(
        grid, DiffusionParameters{rate, dividend, volatility, theta}, old, boundary,
        [&](std::vector<double>& layer, double tau, double elapsed) {
            if (!active(elapsed)) return;
            for (int index = 0; index <= asset_steps; ++index)
                if (knocked(spacing * index)) layer[index] = rebate_value(tau);
        },
        constraint);
    if (!marched) return std::unexpected(marched.error());
    return space->interpolate(old, spot);
}
}
result<PricingResult> FiniteDifferenceBarrierEngine::price(const BarrierOption& option, const PricingContext& context) const
{
    auto settings_valid = validate_finite_difference_settings(settings_);
    if (!settings_valid) return std::unexpected(settings_valid.error());
    if (settings_.asset_steps > 10'000 || settings_.time_steps > 100'000)
        return std::unexpected(Error{error_category::invalid_parameter, "finite-difference grid dimensions are out of range"});
    auto valid = validate_life(context.valuation_time(), option.effective(), option.expiry());
    if (!valid) return std::unexpected(valid.error());
    if (option.observation() == observation_mode::scheduled) {
        auto schedule = validate_schedule(option.schedule(), option.effective(), option.expiry(), context.calendar());
        if (!schedule) return std::unexpected(schedule.error());
    }
    const auto& terms = option.barrier_terms();
    const bool knock_in = terms.is_knock_in();
    const bool touched = terms.breaches(context.asset_price());
    const bool observed_now = terms.monitors(context.valuation_time());
    const double t = actual_365(context.valuation_time(), option.expiry());
    const auto vanilla_price = [&]() -> result<double> {
        auto vanilla = price_at_volatility(*make_european_option(option.type(), option.strike(), option.effective(), option.expiry()),
                                           context, context.parameters().volatility(), risk_measure_output::price_only);
        if (!vanilla) return std::unexpected(vanilla.error());
        return vanilla->get(risk_measure::price).value();
    };
    if (touched && observed_now) {
        if (!knock_in)
            return PricingResult{{risk_measure::price, option.rebate_payment() == rebate_timing::at_hit ? option.rebate() : option.rebate() * std::exp(-context.parameters().risk_free_rate() * t)}};
        auto vanilla = vanilla_price();
        if (!vanilla) return std::unexpected(vanilla.error());
        return PricingResult{{risk_measure::price, *vanilla}};
    }
    auto out = knockout_fd(option, context, settings_); if (!out) return std::unexpected(out.error());
    if (!knock_in) return PricingResult{{risk_measure::price, *out}};
    auto vanilla = vanilla_price();
    if (!vanilla) return std::unexpected(vanilla.error());
    return PricingResult{{risk_measure::price,
                          *vanilla - *out + option.rebate() * std::exp(-context.parameters().risk_free_rate() * t)}};
}
} // namespace kiyosi
