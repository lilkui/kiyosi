#include <kiyosi/pricing/engines/barrier/finite_difference.hpp>

#include <kiyosi/pricing/detail/numerical_greeks.hpp>

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
template <bool scheduled>
Result<PricingResult> barrier_fd(const BarrierOption& option, const PricingContext& context,
                                 FiniteDifferenceSettings settings, GreeksRequest output, double spot_shift)
{
    const double time_to_expiry = actual_365_fixed_year_fraction(context.valuation_time(), option.expiry_date());
    const double spot = context.spot_price(), strike = option.strike();
    const bool knock_in = option.barrier_terms().is_knock_in();
    const bool paired = knock_in && scheduled;
    if (time_to_expiry == 0.0) return make_pricing_result(knock_in ? option.rebate() : std::max((option.option_type() == OptionType::call ? spot - strike : strike - spot), 0.0));
    const double rate = context.model_parameters().risk_free_rate(), dividend = context.model_parameters().dividend_yield(), volatility = context.model_parameters().volatility();
    const double barrier = option.barrier_level();
    const auto space = make_spatial_grid(settings, default_finite_difference_upper_boundary(option, context), {spot, strike, barrier});
    if (!space) return std::unexpected(space.error());
    const double upper = space->upper, spacing = space->spacing;
    const int time_step_count = settings.time_step_count;
    const double theta = scheme_theta(settings.scheme);
    const bool upper_barrier = option.barrier_terms().is_up();
    std::vector<double> observation_times;
    std::vector<Date> observation_dates;
    if (scheduled) {
        for (auto value : option.observation_dates()) {
            if (value < context.valuation_time()) continue;
            const double event_time = actual_365_fixed_year_fraction(context.valuation_time(), value);
            observation_times.push_back(event_time);
            observation_dates.push_back(value);
        }
    }
    const auto grid = !scheduled
                          ? make_finite_difference_time_grid(time_to_expiry, time_step_count)
                          : make_finite_difference_time_grid(context.valuation_time(), option.expiry_date(), time_step_count, std::move(observation_dates));
    if (auto stable = check_explicit_stability(settings.scheme, grid, volatility, rate, dividend, settings.asset_step_count);
        !stable)
        return std::unexpected(stable.error());
    const int asset_step_count = [&] {
        if (scheduled || knock_in || !upper_barrier) return settings.asset_step_count;
        auto first_fixed = static_cast<int>(std::clamp(std::ceil(barrier / spacing), 3.0, static_cast<double>(settings.asset_step_count)));
        if (first_fixed < settings.asset_step_count && spacing * first_fixed < barrier) ++first_fixed;
        return first_fixed;
    }();
    auto active = [&](double time) { return !scheduled || std::ranges::binary_search(observation_times, time); };
    auto payoff = [&](double asset) { return std::max((option.option_type() == OptionType::call ? asset - strike : strike - asset), 0.0); };
    auto knocked = [&](double asset) { return upper_barrier ? asset >= barrier : asset <= barrier; };
    double cached_rebate_tau = -1.0;
    double cached_rebate = 0.0;
    const auto rebate_value = [&](double tau) {
        if (tau != cached_rebate_tau) {
            cached_rebate_tau = tau;
            cached_rebate = option.rebate_timing() == RebateTiming::at_hit ? option.rebate() : scaled_exponential(option.rebate(), -rate * tau);
        }
        return cached_rebate;
    };
    std::vector<double> old(static_cast<std::size_t>(asset_step_count) + 1);
    for (std::size_t index = 0; index < old.size(); ++index)
        old[index] = payoff(spacing * static_cast<double>(index));
    const auto vanilla_boundary = [&](double tau) {
        const double sign = option.option_type() == OptionType::call ? 1.0 : -1.0;
        const auto probabilities = black_scholes_probabilities(sign, upper, strike, rate, dividend, volatility, tau);
        return Boundaries{option.option_type() == OptionType::put ? scaled_exponential(strike, -rate * tau) : 0.0,
                          black_scholes_values(sign, upper, strike, -dividend * tau,
                                               -rate * tau, probabilities)
                              .price};
    };
    std::vector<double> vanilla;
    if (knock_in) {
        // In/out parity must use the same space and time grids before interpolation.
        vanilla = old;
        if (!paired) {
            const auto marched = march_backward(grid, DiffusionParameters{rate, dividend, volatility, theta}, vanilla, vanilla_boundary);
            if (!marched) return std::unexpected(marched.error());
        }
    }
    if (active(time_to_expiry))
        for (std::size_t index = 0; index < old.size(); ++index)
            if (knocked(spacing * static_cast<double>(index))) old[index] = option.rebate();
    const auto boundary = [&](double time, double tau) {
        if (!scheduled && upper_barrier)
            return Boundaries{option.option_type() == OptionType::put ? scaled_exponential(strike, -rate * tau) : 0.0,
                              rebate_value(tau)};
        Boundaries edges = vanilla_boundary(tau);
        const auto observation = std::ranges::lower_bound(observation_times, time);
        if (!scheduled || observation != observation_times.end()) {
            // Scheduled edges settle at the next observation, not between observations.
            const double rebate = scheduled && option.rebate_timing() == RebateTiming::at_hit
                                      ? scaled_exponential(option.rebate(), -rate * (*observation - time))
                                      : rebate_value(tau);
            if (knocked(0.0)) edges.lower = rebate;
            if (knocked(upper)) edges.upper = rebate;
        }
        return edges;
    };
    const auto constraint = [&](int index, double tau) -> std::optional<double> {
        if (!scheduled && knocked(spacing * index))
            return rebate_value(tau);
        return std::nullopt;
    };
    std::vector<double> next(old.size()), next_vanilla(paired ? old.size() : 0);
    FiniteDifferenceStep stepper(old.size());
    const DiffusionParameters parameters{rate, dividend, volatility, theta};
    for (std::size_t step = grid.time_steps.size(); step-- > 0;) {
        const double tau = time_to_expiry - grid.times[step];
        const Boundaries edges = boundary(grid.times[step], tau);
        const bool advanced = paired
                                  ? stepper.advance_pair(vanilla, next_vanilla, old, next, grid.time_steps[step], parameters, vanilla_boundary(tau), edges)
                              : scheduled
                                  ? stepper.advance(old, next, grid.time_steps[step], parameters, edges.lower, edges.upper)
                                  : stepper.advance(old, next, grid.time_steps[step], parameters, edges.lower, edges.upper,
                                                    [&](int index) { return constraint(index, tau); });
        if (!advanced)
            return std::unexpected(Error{ErrorCategory::invalid_result,
                                         "finite-difference system is numerically unstable"});
        if (scheduled && active(grid.times[step]))
            for (std::size_t index = 0; index < next.size(); ++index)
                if (knocked(spacing * static_cast<double>(index))) next[index] = rebate_value(tau);
        old.swap(next);
        if (paired) vanilla.swap(next_vanilla);
    }
    if (knock_in) {
        const double rebate = scaled_exponential(option.rebate(), -rate * time_to_expiry);
        for (std::size_t index = 0; index < old.size(); ++index)
            old[index] = std::max(vanilla[index] - old[index] + rebate, 0.0);
    }
    // Preserve full-grid spline curvature and Greeks after solving only the live region.
    if (old.size() < space->size()) old.resize(space->size(), rebate_value(time_to_expiry));
    std::vector<double> curvature;
    const double value = space->interpolate(old, spot, curvature);
    return finite_difference_result(value, spot, space->upper, output, spot_shift, [&](double shifted_spot) { return checked_price(space->interpolate(old, shifted_spot, curvature)); }, [&](double shifted_spot) { return space->delta(old, shifted_spot); }, [&](double shifted_spot) { return space->gamma(old, shifted_spot); });
}
Result<PricingResult> price_barrier(
    const BarrierOption& option, const PricingContext& context, FiniteDifferenceSettings settings,
    bool& schedule_validated, GreeksRequest output, double spot_shift = NumericalShiftSettings{}.spot_shift)
{
    auto valid = validate_valuation_within_instrument_life(context.valuation_time(), option.effective_date(), option.expiry_date());
    if (!valid) return std::unexpected(valid.error());
    if (!schedule_validated && option.observation_mode() == ObservationMode::scheduled) {
        auto schedule = validate_observation_dates(option.observation_dates(), option.effective_date(), option.expiry_date(), context.calendar());
        if (!schedule) return std::unexpected(schedule.error());
        schedule_validated = true;
    }
    if (const auto settled = resolved_barrier_price(option, context)) {
        if (!*settled) return std::unexpected(settled->error());
        return make_pricing_result(**settled);
    }
    return option.observation_mode() == ObservationMode::scheduled
               ? barrier_fd<true>(option, context, settings, output, spot_shift)
               : barrier_fd<false>(option, context, settings, output, spot_shift);
}

} // namespace

Result<double> FiniteDifferenceBarrierEngine::price(const BarrierOption& option, const PricingContext& context) const
{
    bool schedule_validated = false;
    return detail::price_value(price_barrier(option, context, settings_, schedule_validated, GreeksRequest{}));
}
Result<PricingResult> FiniteDifferenceBarrierEngine::price_with_greeks(const BarrierOption& option, const PricingContext& context,
                                                                       GreeksRequest greeks, NumericalShiftSettings settings) const
{
    bool schedule_validated = false;
    return detail::price_with_greeks(*this, option, context, greeks, settings,
                                     [&](const auto& engine, const PricingContext& shifted, GreeksRequest request) {
                                         return price_barrier(option, shifted, engine.settings(), schedule_validated, request, settings.spot_shift);
                                     });
}

} // namespace kiyosi
