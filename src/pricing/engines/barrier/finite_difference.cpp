#include <kiyosi/pricing/engines/barrier/finite_difference.hpp>

#include <kiyosi/pricing/numerical_greeks.hpp>

#include <algorithm>
#include <cmath>
#include <vector>

#include "../../detail/barrier_settlement.hpp"
#include "../../detail/fd_grid.hpp"
#include "../../detail/fd_scheme.hpp"
#include "../../detail/math.hpp"

namespace kiyosi {
using namespace detail;
namespace {
// Valuation time and observation dates have been validated by price.
Result<PricingResult> barrier_fd(const BarrierOption& option, const PricingContext& context,
                                 FiniteDifferenceSettings settings, GreeksRequest output)
{
    const double time_to_expiry = actual_365_fixed_year_fraction(context.valuation_time(), option.expiry_date());
    const double spot = context.spot_price(), strike = option.strike();
    const bool knock_in = option.barrier_terms().is_knock_in();
    if (time_to_expiry == 0.0) return make_pricing_result(knock_in ? option.rebate() : std::max((option.option_type() == OptionType::call ? spot - strike : strike - spot), 0.0));
    const double rate = context.model_parameters().risk_free_rate(), dividend = context.model_parameters().dividend_yield(), volatility = context.model_parameters().volatility();
    const double barrier = option.barrier_level();
    const int asset_step_count = settings.asset_step_count;
    const auto space = make_spatial_grid(settings, default_finite_difference_upper_boundary(option, context), {spot, strike, barrier});
    if (!space) return std::unexpected(space.error());
    const double upper = space->upper, spacing = space->spacing;
    const int time_step_count = settings.time_step_count;
    const double theta = scheme_theta(settings.scheme);
    const bool upper_barrier = option.barrier_terms().is_up();
    std::vector<double> observation_times;
    if (option.observation_mode() == ObservationMode::scheduled) {
        for (auto value : option.observation_dates()) {
            if (value < context.valuation_time()) continue;
            const double event_time = actual_365_fixed_year_fraction(context.valuation_time(), value);
            observation_times.push_back(event_time);
        }
    }
    const auto grid = make_finite_difference_time_grid(time_to_expiry, time_step_count, observation_times);
    if (auto stable = check_explicit_stability(settings.scheme, grid, volatility, rate, dividend, asset_step_count);
        !stable)
        return std::unexpected(stable.error());
    auto active = [&](double time) { return option.observation_mode() == ObservationMode::continuous || std::ranges::binary_search(observation_times, time); };
    auto payoff = [&](double asset) { return std::max((option.option_type() == OptionType::call ? asset - strike : strike - asset), 0.0); };
    auto knocked = [&](double asset) { return upper_barrier ? asset >= barrier : asset <= barrier; };
    double cached_rebate_tau = -1.0;
    double cached_rebate = 0.0;
    const auto rebate_value = [&](double tau) {
        if (tau != cached_rebate_tau) {
            cached_rebate_tau = tau;
            cached_rebate = option.rebate_timing() == RebateTiming::at_hit ? option.rebate() : option.rebate() * std::exp(-rate * tau);
        }
        return cached_rebate;
    };
    std::vector<double> old(space->size());
    for (int index = 0; index <= asset_step_count; ++index)
        old[index] = payoff(spacing * index);
    const auto vanilla_boundary = [&](double tau) {
        const double sign = option.option_type() == OptionType::call ? 1.0 : -1.0;
        const auto probabilities = black_scholes_probabilities(sign, upper, strike, rate, dividend, volatility, tau);
        return Boundaries{option.option_type() == OptionType::put ? strike * std::exp(-rate * tau) : 0.0,
                          sign * (upper * std::exp(-dividend * tau) * probabilities.asset -
                                  strike * std::exp(-rate * tau) * probabilities.cash)};
    };
    std::vector<double> vanilla;
    if (knock_in) {
        // In/out parity must use the same space and time grids before interpolation.
        vanilla = old;
        const auto marched = march_backward(grid, DiffusionParameters{rate, dividend, volatility, theta}, vanilla, vanilla_boundary);
        if (!marched) return std::unexpected(marched.error());
    }
    if (active(time_to_expiry))
        for (int index = 0; index <= asset_step_count; ++index)
            if (knocked(spacing * index)) old[index] = option.rebate();
    const auto boundary = [&](double tau) {
        Boundaries edges = vanilla_boundary(tau);
        if (option.observation_mode() == ObservationMode::continuous) {
            if (knocked(0.0)) edges.lower = rebate_value(tau);
            if (knocked(upper)) edges.upper = rebate_value(tau);
        }
        return edges;
    };
    const auto constraint = [&](int index, double tau) -> std::optional<double> {
        if (option.observation_mode() == ObservationMode::continuous && knocked(spacing * index))
            return rebate_value(tau);
        return std::nullopt;
    };
    const auto marched = march_backward(
        grid, DiffusionParameters{rate, dividend, volatility, theta}, old, boundary,
        [&](std::vector<double>& layer, double tau, double elapsed) {
            if (!active(elapsed)) return;
            for (int index = 0; index <= asset_step_count; ++index)
                if (knocked(spacing * index)) layer[index] = rebate_value(tau);
        },
        constraint);
    if (!marched) return std::unexpected(marched.error());
    if (knock_in) {
        const double rebate = option.rebate() * std::exp(-rate * time_to_expiry);
        for (std::size_t index = 0; index < old.size(); ++index)
            old[index] = std::max(vanilla[index] - old[index] + rebate, 0.0);
    }
    return make_pricing_result(space->interpolate(old, spot),
                               {{Greek::delta, output.has(Greek::delta) ? std::optional{space->delta(old, spot)} : std::nullopt},
                                {Greek::gamma, output.has(Greek::gamma) ? std::optional{space->gamma(old, spot)} : std::nullopt}});
}
} // namespace
Result<PricingResult> FiniteDifferenceBarrierEngine::price_native(
    const BarrierOption& option, const PricingContext& context, GreeksRequest output) const
{
    auto settings_valid = detail::validate_finite_difference_settings(
        settings_, general_fd_max_asset_steps, general_fd_max_time_steps);
    if (!settings_valid) return std::unexpected(settings_valid.error());
    auto valid = validate_valuation_within_instrument_life(context.valuation_time(), option.effective_date(), option.expiry_date());
    if (!valid) return std::unexpected(valid.error());
    if (option.observation_mode() == ObservationMode::scheduled) {
        auto schedule = validate_observation_dates(option.observation_dates(), option.effective_date(), option.expiry_date(), context.calendar());
        if (!schedule) return std::unexpected(schedule.error());
    }
    if (const auto settled = resolved_barrier_price(option, context)) {
        if (!*settled) return std::unexpected(settled->error());
        return make_pricing_result(**settled);
    }
    return barrier_fd(option, context, settings_, output);
}
Result<double> FiniteDifferenceBarrierEngine::price(const BarrierOption& option, const PricingContext& context) const
{
    return detail::price_value(price_native(option, context, GreeksRequest{}));
}
Result<PricingResult> FiniteDifferenceBarrierEngine::price_with_greeks(const BarrierOption& option, const PricingContext& context,
                                                                       GreeksRequest greeks, NumericalShiftSettings settings) const
{
    return detail::price_with_greeks(*this, option, context, greeks, settings,
                                     [&](const auto& engine) {
                                         const auto output = detail::at_spot_discontinuity(option, context, detail::symmetric_shift(context.spot_price(), settings.spot_shift)) ? GreeksRequest{} : greeks;
                                         return engine.price_native(option, context, output);
                                     });
}

} // namespace kiyosi
