#include <kiyosi/pricing/engines/barrier/finite_difference.hpp>
#include <kiyosi/pricing/engines/barrier/double_finite_difference.hpp>
#include <kiyosi/pricing/engines/binary_barrier/double_finite_difference.hpp>

#include "../../detail/numerical_greeks.hpp"

#include <algorithm>
#include <array>
#include <cmath>
#include <vector>

#include "../../detail/barrier_settlement.hpp"
#include "../../detail/fd_event.hpp"
#include "../../detail/fd_grid.hpp"
#include "../../detail/fd_scheme.hpp"
#include "../../detail/math.hpp"

namespace kiyosi {
using namespace detail;
namespace {
template <typename Option>
Date barrier_pricing_expiry(const Option& option)
{
    if constexpr (std::same_as<Option, DoubleTouchOption>)
        if (option.settlement_timing() == SettlementTiming::at_hit && option.observation_mode() == ObservationMode::scheduled)
            return option.observation_dates().back();
    return option.expiry_date();
}

// Valuation time and observation dates have been validated by price.
template <bool scheduled, typename Option>
Result<PricingResult> barrier_fd(const Option& option, const PricingContext& context,
                                 FiniteDifferenceSettings settings, GreeksRequest output, double spot_shift)
{
    constexpr bool cash_touch = std::same_as<Option, DoubleTouchOption>;
    constexpr bool double_barrier = cash_touch || std::same_as<Option, DoubleBarrierOption>;
    const double time_to_expiry = actual_365_fixed_year_fraction(context.valuation_time(), barrier_pricing_expiry(option));
    const double spot = context.spot_price();
    const double payoff_level = [&] {
        if constexpr (cash_touch) return spot;
        else return option.strike();
    }();
    const bool knock_in = !cash_touch && option.barrier_terms().is_knock_in();
    const double rebate = [&] {
        if constexpr (cash_touch) return option.is_one_touch() ? option.payout() : 0.0;
        else return option.rebate();
    }();
    const auto rebate_timing = [&] {
        if constexpr (cash_touch) return option.settlement_timing() == SettlementTiming::at_hit ? RebateTiming::at_hit : RebateTiming::at_expiry;
        else return option.rebate_timing();
    }();
    const auto payoff = [&](double asset) {
        if constexpr (cash_touch) return option.is_one_touch() ? 0.0 : option.payout();
        else return std::max((option.option_type() == OptionType::call ? asset - payoff_level : payoff_level - asset), 0.0);
    };
    if (time_to_expiry == 0.0) return make_pricing_result(knock_in ? rebate : payoff(spot));
    const auto& market = context.model_parameters();
    const double rate = curve_forward_rate(market.risk_free_curve(), context.valuation_time(), 0.0);
    const double dividend = curve_forward_rate(market.dividend_curve(), context.valuation_time(), 0.0);
    const double volatility = market.volatility();
    const auto rate_discount = [&](double tau) {
        return curve_log_factor(market.risk_free_curve(), context.valuation_time(), time_to_expiry - tau, time_to_expiry);
    };
    const auto dividend_discount = [&](double tau) {
        return curve_log_factor(market.dividend_curve(), context.valuation_time(), time_to_expiry - tau, time_to_expiry);
    };
    const auto levels = [&] {
        if constexpr (double_barrier) return std::array{option.lower_barrier(), option.upper_barrier()};
        else return std::array{option.barrier_level()};
    }();
    auto space = [&] {
        if constexpr (double_barrier)
            return make_spatial_grid(settings, default_finite_difference_upper_boundary(option, context), {spot, payoff_level, levels[0], levels[1]});
        else return make_spatial_grid(settings, default_finite_difference_upper_boundary(option, context), {spot, payoff_level, levels[0]});
    }();
    if (!space) return std::unexpected(space.error());
    if constexpr (double_barrier && !scheduled) {
        // Fit both absorbing boundaries rather than rounding away a narrow live interval.
        space->upper = levels[1];
        space->nodes.reserve(space->size());
        for (std::size_t index = 0; index < space->size(); ++index) {
            const double asset = std::lerp(levels[0], levels[1], static_cast<double>(index) / settings.asset_step_count);
            if (!space->nodes.empty() && asset <= space->nodes.back())
                return std::unexpected(Error{ErrorCategory::invalid_parameter, "double-barrier grid nodes are not representably distinct"});
            space->nodes.push_back(asset);
        }
    }
    const double upper = space->upper, spacing = space->spacing;
    const int time_step_count = settings.time_step_count;
    const double theta = scheme_theta(settings.scheme);
    const bool upper_barrier = [&] {
        if constexpr (double_barrier) return false;
        else return option.barrier_terms().is_up();
    }();
    std::vector<double> observation_times;
    std::vector<Date> observation_dates;
    if (scheduled) {
        for (auto value : option.observation_dates()) {
            // Current observations have already been resolved against the actual spot.
            if (value <= context.valuation_time()) continue;
            const double event_time = actual_365_fixed_year_fraction(context.valuation_time(), value);
            observation_times.push_back(event_time);
            observation_dates.push_back(value);
        }
    }
    const auto grid = curve_finite_difference_time_grid(context, barrier_pricing_expiry(option), time_step_count, std::move(observation_dates));
    const DiffusionParameters parameters{rate, dividend, volatility, theta, 0.0, space->nodes.empty() ? nullptr : &*space};
    if (auto stable = check_curve_explicit_stability(settings.scheme, grid, parameters, settings.asset_step_count, context);
        !stable)
        return std::unexpected(stable.error());
    const int asset_step_count = [&] {
        if (scheduled || double_barrier || knock_in || !upper_barrier) return settings.asset_step_count;
        auto first_fixed = static_cast<int>(std::clamp(std::ceil(levels[0] / spacing), 3.0, static_cast<double>(settings.asset_step_count)));
        if (first_fixed < settings.asset_step_count && spacing * first_fixed < levels[0]) ++first_fixed;
        return first_fixed;
    }();
    auto active = [&](double time) { return !scheduled || std::ranges::binary_search(observation_times, time); };
    auto knocked = [&](double asset) { return option.barrier_terms().is_breached_by(asset); };
    double cached_rebate_tau = -1.0;
    double cached_rebate = 0.0;
    const auto rebate_value = [&](double tau) {
        if (tau != cached_rebate_tau) {
            cached_rebate_tau = tau;
            cached_rebate = rebate_timing == RebateTiming::at_hit ? rebate : scaled_exponential(rebate, rate_discount(tau));
        }
        return cached_rebate;
    };
    std::vector<double> old(static_cast<std::size_t>(asset_step_count) + 1);
    for (std::size_t index = 0; index < old.size(); ++index)
        old[index] = payoff(space->asset_at(index));
    const auto vanilla_boundary = [&](double tau) {
        const double effective_rate = tau == 0.0 ? rate : -rate_discount(tau) / tau;
        const double effective_dividend = tau == 0.0 ? dividend : -dividend_discount(tau) / tau;
        if constexpr (cash_touch) {
            const double value = scaled_exponential(payoff(spot), rate_discount(tau));
            return Boundaries{value, value};
        } else {
            const double sign = option.option_type() == OptionType::call ? 1.0 : -1.0;
            const auto probabilities = black_scholes_probabilities(sign, upper, payoff_level, effective_rate, effective_dividend, volatility, tau);
            const double lower_value = [&] {
                if constexpr (double_barrier && !scheduled) {
                    const auto lower_probabilities = black_scholes_probabilities(sign, levels[0], payoff_level, effective_rate, effective_dividend, volatility, tau);
                    return black_scholes_values(sign, levels[0], payoff_level, dividend_discount(tau), rate_discount(tau), lower_probabilities).price;
                } else return option.option_type() == OptionType::put ? scaled_exponential(payoff_level, rate_discount(tau)) : 0.0;
            }();
            return Boundaries{lower_value,
                              black_scholes_values(sign, upper, payoff_level, dividend_discount(tau),
                                                   rate_discount(tau), probabilities)
                                  .price};
        }
    };
    std::vector<double> vanilla;
    if (knock_in) {
        // Keep the vanilla reference on its original schedule when barrier events restart damping.
        vanilla = old;
        const auto marched = march_backward<true>(grid, parameters, vanilla, vanilla_boundary, [](std::vector<double>&, double, double) {}, nullptr, &context);
        if (!marched) return std::unexpected(marched.error());
    }
    const auto event_breaks = levels;
    std::array<double, levels.size() + 1> terminal_breaks{};
    std::ranges::copy(levels, terminal_breaks.begin());
    terminal_breaks.back() = payoff_level;
    std::ranges::sort(terminal_breaks);
    const auto observe = [&](double tau, bool terminal) {
        std::array<std::size_t, levels.size()> cuts{};
        std::array<double, levels.size()> projected{};
        const double event_value = rebate_value(tau);
        for (std::size_t index = 0; index < levels.size(); ++index) {
            const std::size_t cut = space->nearest_node(levels[index]);
            cuts[index] = cut;
            // Project the full double-barrier event to preserve nonnegative continuation at every grid placement.
            projected[index] = terminal || double_barrier
                                   ? project_finite_difference_event<1>(*space, terminal_breaks, cut, [&](double asset) {
                                         return std::array{knocked(asset) ? event_value : (terminal ? payoff(asset) : space->linear_interpolate(old, asset))};
                                     })[0]
                                   : old[cut] + project_finite_difference_event<1>(*space, event_breaks, cut, [&](double asset) {
                                         return std::array{knocked(asset) ? event_value - space->linear_interpolate(old, asset) : 0.0};
                                     })[0];
        }
        for (std::size_t index = 0; index < old.size(); ++index)
            if (knocked(space->asset_at(index))) old[index] = event_value;
        for (std::size_t index = 0; index < cuts.size(); ++index)
            old[cuts[index]] = projected[index];
    };
    if (active(time_to_expiry)) {
        if (scheduled) observe(0.0, true);
        else
            for (std::size_t index = 0; index < old.size(); ++index)
                if (knocked(space->asset_at(index))) old[index] = rebate;
    }
    const auto boundary = [&](double time, double tau) {
        if constexpr (double_barrier && !scheduled) {
            const double event_value = rebate_value(tau);
            return Boundaries{event_value, event_value};
        }
        if constexpr (!double_barrier) {
            if (!scheduled && upper_barrier)
                return Boundaries{option.option_type() == OptionType::put ? scaled_exponential(payoff_level, rate_discount(tau)) : 0.0, rebate_value(tau)};
        }
        Boundaries edges = vanilla_boundary(tau);
        const auto observation = std::ranges::lower_bound(observation_times, time);
        if (!scheduled || observation != observation_times.end()) {
            // Scheduled edges settle at the next observation, not between observations.
            const double event_value = scheduled && rebate_timing == RebateTiming::at_hit
                                           ? scaled_exponential(rebate, curve_log_factor(market.risk_free_curve(), context.valuation_time(), time, *observation))
                                           : rebate_value(tau);
            if (knocked(space->asset_at(0))) edges.lower = event_value;
            if (knocked(upper)) edges.upper = event_value;
        }
        return edges;
    };
    const auto constraint = [&](int index, double tau) -> std::optional<double> {
        if (!scheduled && knocked(space->asset_at(static_cast<std::size_t>(index))))
            return rebate_value(tau);
        return std::nullopt;
    };
    std::vector<double> next(old.size());
    FiniteDifferenceStep stepper(old.size());
    RannacherDamping damping{2};
    for (std::size_t step = grid.time_steps.size(); step-- > 0;) {
        const auto substeps = damping.next(theta);
        auto step_parameters = parameters;
        step_parameters.theta = substeps.theta;
        const double dt = grid.time_steps[step] / substeps.count;
        for (int substep = 0; substep < substeps.count; ++substep) {
            const double offset = dt * (substeps.count - substep - 1);
            const double time = grid.times[step] + offset;
            const double tau = time_to_expiry - grid.times[step] - offset;
            step_parameters = diffusion_interval(step_parameters, context, time, time + dt);
            const Boundaries edges = boundary(time, tau);
            const bool advanced = scheduled || double_barrier
                                      ? stepper.advance(old, next, dt, step_parameters, edges.lower, edges.upper)
                                      : stepper.advance(old, next, dt, step_parameters, edges.lower, edges.upper,
                                                        [&](int index) { return constraint(index, tau); });
            if (!advanced)
                return std::unexpected(Error{ErrorCategory::invalid_result,
                                             "finite-difference system is numerically unstable"});
            old.swap(next);
        }
        if (scheduled && active(grid.times[step])) {
            observe(time_to_expiry - grid.times[step], false);
            damping.restart();
        }
    }
    if (knock_in) {
        const double discounted_rebate = scaled_exponential(rebate, rate_discount(time_to_expiry));
        for (std::size_t index = 0; index < old.size(); ++index)
            old[index] = std::max(vanilla[index] - old[index] + discounted_rebate, 0.0);
    }
    // Preserve full-grid spline curvature and Greeks after solving only the live region.
    if (old.size() < space->size()) old.resize(space->size(), rebate_value(time_to_expiry));
    std::vector<double> curvature;
    const double value = space->interpolate(old, spot, curvature);
    return finite_difference_result(value, spot, space->upper, output, spot_shift, [&](double shifted_spot) { return checked_price(space->interpolate(old, shifted_spot, curvature)); }, [&](double shifted_spot) { return space->delta(old, shifted_spot, curvature); }, [&](double shifted_spot) { return space->gamma(old, shifted_spot, curvature); });
}
template <typename Option>
Result<PricingResult> price_barrier(
    const Option& option, const PricingContext& context, FiniteDifferenceSettings settings,
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
    if (const auto covered = validate_curve_horizon(context, start_of_day(barrier_pricing_expiry(option))); !covered)
        return std::unexpected(covered.error());
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

Result<double> FiniteDifferenceDoubleBarrierEngine::price(const DoubleBarrierOption& option, const PricingContext& context) const
{
    bool schedule_validated = false;
    return detail::price_value(price_barrier(option, context, settings_, schedule_validated, GreeksRequest{}));
}

Result<PricingResult> FiniteDifferenceDoubleBarrierEngine::price_with_greeks(const DoubleBarrierOption& option, const PricingContext& context,
                                                                             GreeksRequest greeks, NumericalShiftSettings settings) const
{
    bool schedule_validated = false;
    return detail::price_with_greeks(*this, option, context, greeks, settings,
                                     [&](const auto& engine, const PricingContext& shifted, GreeksRequest request) {
                                         return price_barrier(option, shifted, engine.settings(), schedule_validated, request, settings.spot_shift);
                                     });
}

Result<double> FiniteDifferenceDoubleTouchEngine::price(const DoubleTouchOption& option, const PricingContext& context) const
{
    bool schedule_validated = false;
    return detail::price_value(price_barrier(option, context, settings_, schedule_validated, GreeksRequest{}));
}

Result<PricingResult> FiniteDifferenceDoubleTouchEngine::price_with_greeks(const DoubleTouchOption& option, const PricingContext& context,
                                                                           GreeksRequest greeks, NumericalShiftSettings settings) const
{
    bool schedule_validated = false;
    return detail::price_with_greeks(*this, option, context, greeks, settings,
                                     [&](const auto& engine, const PricingContext& shifted, GreeksRequest request) {
                                         return price_barrier(option, shifted, engine.settings(), schedule_validated, request, settings.spot_shift);
                                     });
}

} // namespace kiyosi
