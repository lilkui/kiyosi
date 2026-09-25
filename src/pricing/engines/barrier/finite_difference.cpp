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
Result<double> knockout_fd(const BarrierOption& option, const PricingContext& context, FiniteDifferenceSettings settings)
{
    auto valid = validate_valuation_within_instrument_life(context.valuation_time(), option.effective_date(), option.expiry_date());
    if (!valid) return std::unexpected(valid.error());
    if (option.observation_mode() == ObservationMode::scheduled) {
        auto schedule = validate_observation_dates(option.observation_dates(), option.effective_date(), option.expiry_date(), context.calendar());
        if (!schedule) return std::unexpected(schedule.error());
    }
    const double time_to_expiry = actual_365_fixed_year_fraction(context.valuation_time(), option.expiry_date());
    const double spot = context.spot_price(), strike = option.strike();
    if (time_to_expiry == 0.0) return std::max((option.option_type() == OptionType::call ? spot - strike : strike - spot), 0.0);
    const double rate = context.model_parameters().risk_free_rate(), dividend = context.model_parameters().dividend_yield(), volatility = context.model_parameters().volatility();
    const double barrier = option.barrier_level();
    const int asset_step_count = settings.asset_step_count;
    const auto space = make_spatial_grid(settings, std::max(4.0 * strike, 4.0 * spot), {spot, strike, barrier});
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
            if (event_time >= 0.0 && event_time <= time_to_expiry) observation_times.push_back(event_time);
        }
    }
    const auto grid = make_finite_difference_time_grid(time_to_expiry, time_step_count, observation_times);
    if (auto stable = check_explicit_stability(settings.scheme, grid, volatility, rate, asset_step_count);
        !stable)
        return std::unexpected(stable.error());
    auto active = [&](double time) { return option.observation_mode() == ObservationMode::continuous || std::binary_search(observation_times.begin(), observation_times.end(), time); };
    std::sort(observation_times.begin(), observation_times.end());
    auto payoff = [&](double asset) { return std::max((option.option_type() == OptionType::call ? asset - strike : strike - asset), 0.0); };
    auto knocked = [&](double asset) { return upper_barrier ? asset >= barrier : asset <= barrier; };
    auto rebate_value = [&](double tau) { return option.rebate_timing() == RebateTiming::at_hit ? option.rebate() : option.rebate() * std::exp(-rate * tau); };
    std::vector<double> old(space->size());
    for (int index = 0; index <= asset_step_count; ++index)
        old[index] = payoff(spacing * index);
    if (active(time_to_expiry))
        for (int index = 0; index <= asset_step_count; ++index)
            if (knocked(spacing * index)) old[index] = option.rebate();
    const auto boundary = [&](double tau) {
        Boundaries edges{option.option_type() == OptionType::put ? strike * std::exp(-rate * tau) : 0.0,
                         option.option_type() == OptionType::call ? upper * std::exp(-dividend * tau) - strike * std::exp(-rate * tau) : 0.0};
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
    return space->interpolate(old, spot);
}
} // namespace
Result<PricingResult> FiniteDifferenceBarrierEngine::price_native(const BarrierOption& option, const PricingContext& context) const
{
    auto settings_valid = validate_finite_difference_settings(settings_);
    if (!settings_valid) return std::unexpected(settings_valid.error());
    if (settings_.asset_step_count > general_fd_max_asset_steps || settings_.time_step_count > general_fd_max_time_steps)
        return std::unexpected(Error{ErrorCategory::invalid_parameter, "finite-difference grid dimensions are out of range"});
    auto valid = validate_valuation_within_instrument_life(context.valuation_time(), option.effective_date(), option.expiry_date());
    if (!valid) return std::unexpected(valid.error());
    if (option.observation_mode() == ObservationMode::scheduled) {
        auto schedule = validate_observation_dates(option.observation_dates(), option.effective_date(), option.expiry_date(), context.calendar());
        if (!schedule) return std::unexpected(schedule.error());
    }
    const auto& terms = option.barrier_terms();
    const auto prior_touch = terms.was_touched_before(context.valuation_time());
    if (!prior_touch) return std::unexpected(prior_touch.error());
    const bool knock_in = terms.is_knock_in();
    const bool touched = terms.is_breached_by(context.spot_price());
    const bool observed_now = terms.is_monitored_at(context.valuation_time());
    const double t = actual_365_fixed_year_fraction(context.valuation_time(), option.expiry_date());
    const auto vanilla_price = [&]() -> Result<double> {
        auto vanilla = price_at_volatility(*make_european_option(option.option_type(), option.strike(), option.effective_date(), option.expiry_date()),
                                           context, context.model_parameters().volatility(), RiskMeasureOutput::price_only);
        if (!vanilla) return std::unexpected(vanilla.error());
        return *vanilla->require(RiskMeasure::price);
    };
    if (*prior_touch || (touched && observed_now)) {
        if (!knock_in)
            return make_pricing_result(
                {{RiskMeasure::price,
                  option.rebate_timing() == RebateTiming::at_hit
                      ? (*prior_touch ? 0.0 : option.rebate())
                      : option.rebate() *
                            std::exp(-context.model_parameters().risk_free_rate() * t)}});
        auto vanilla = vanilla_price();
        if (!vanilla) return std::unexpected(vanilla.error());
        return make_pricing_result({{RiskMeasure::price, *vanilla}});
    }
    auto out = knockout_fd(option, context, settings_);
    if (!out) return std::unexpected(out.error());
    if (!knock_in) return make_pricing_result({{RiskMeasure::price, *out}});
    auto vanilla = vanilla_price();
    if (!vanilla) return std::unexpected(vanilla.error());
    return make_pricing_result(
        {{RiskMeasure::price,
          *vanilla - *out +
              option.rebate() * std::exp(-context.model_parameters().risk_free_rate() * t)}});
}
} // namespace kiyosi
