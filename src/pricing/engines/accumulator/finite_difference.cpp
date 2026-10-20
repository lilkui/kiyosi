#include <kiyosi/pricing/engines/accumulator/finite_difference.hpp>

#include <kiyosi/pricing/detail/numerical_greeks.hpp>

#include <algorithm>
#include <array>
#include <cmath>
#include <optional>
#include <vector>

#include "../../detail/accumulator_state.hpp"
#include "../../detail/calendar_dates.hpp"
#include "../../detail/fd_damping.hpp"
#include "../../detail/fd_event.hpp"
#include "../../detail/fd_grid.hpp"
#include "../../detail/fd_scheme.hpp"
#include "../../detail/math.hpp"

namespace kiyosi {
using namespace detail;

Result<PricingResult> FiniteDifferenceAccumulatorEngine::price_native(
    const Accumulator& option, const PricingContext& context, GreeksRequest output, double spot_shift) const
{
    auto valid = validate_valuation_within_instrument_life(context.valuation_time(), option.effective_date(), option.expiry_date());
    if (!valid) return std::unexpected(valid.error());
    auto expiry_valid = validate_trading_expiry(context.calendar(), option.expiry_date());
    if (!expiry_valid) return std::unexpected(expiry_valid.error());

    const auto initial = accumulator_initial_state(option, context);
    if (initial.settlement) return make_pricing_result(*initial.settlement);

    const double spot = context.spot_price();

    const double relevant = highest_finite_difference_level(option, context);
    const auto space = make_spatial_grid(settings_, default_finite_difference_upper_boundary(option, context), {relevant});
    if (!space) return std::unexpected(space.error());

    const auto future_trading_dates =
        trading_dates(context.calendar(), context.valuation_time(), option.expiry_date());
    std::vector<double> trading_times;
    trading_times.reserve(future_trading_dates.size());
    for (const Date value : future_trading_dates)
        trading_times.push_back(actual_365_fixed_year_fraction(context.valuation_time(), value));

    const double rate = context.model_parameters().risk_free_rate();
    const double dividend = context.model_parameters().dividend_yield();
    const double sigma = context.model_parameters().volatility();
    const auto grid = make_finite_difference_time_grid(
        context.valuation_time(), option.expiry_date(), settings_.time_step_count, future_trading_dates);
    if (auto stable = check_explicit_stability(settings_.scheme, grid, sigma, rate, dividend, settings_.asset_step_count);
        !stable)
        return std::unexpected(stable.error());

    const std::size_t size = space->size();
    std::vector<double> slope(size), intercept(size), next_slope(size), next_intercept(size);
    const bool has_future_purchases = accumulator_accrues(option);
    std::vector<double> breaks{option.strike(), option.knock_out_level()};
    std::ranges::sort(breaks);
    breaks.erase(std::ranges::unique(breaks).begin(), breaks.end());
    const auto event_value = [&](double asset, double continuation_slope, double continuation_intercept) {
        if (accumulator_knocked_out(asset, option.knock_out_level()))
            return std::array{asset - option.strike(), 0.0};
        const double bought = accumulator_purchase_quantity(asset, option.strike(), option.daily_quantity(), option.acceleration_factor());
        return std::array{continuation_slope, continuation_intercept + continuation_slope * bought};
    };
    for (std::size_t index = 0; index < size; ++index) {
        const auto projected = project_finite_difference_event<2>(*space, breaks, index, [&](double asset) {
            return event_value(asset, asset - option.strike(), 0.0);
        });
        slope[index] = projected[0];
        intercept[index] = projected[1];
    }

    const DiffusionParameters parameters{rate, dividend, sigma, scheme_theta(settings_.scheme)};
    LinearBoundaryStepper stepper(
        size, space->upper, space->spacing, parameters);
    RannacherDamping damping{2};
    for (std::size_t step = grid.time_steps.size(); step-- > 0;) {
        const auto substeps = damping.next(parameters.theta);
        auto step_parameters = parameters;
        step_parameters.theta = substeps.theta;
        stepper.set_parameters(step_parameters);
        const double dt = grid.time_steps[step] / substeps.count;
        for (int substep = 0; substep < substeps.count; ++substep) {
            const bool advanced = has_future_purchases
                                      ? stepper.advance_pair(slope, next_slope, intercept, next_intercept, dt)
                                      : stepper.advance(slope, next_slope, dt);
            if (!advanced)
                return std::unexpected(Error{ErrorCategory::invalid_result,
                                             "finite-difference system is numerically unstable"});
            slope.swap(next_slope);
            if (has_future_purchases) intercept.swap(next_intercept);
        }
        if (std::ranges::binary_search(trading_times, grid.times[step])) {
            for (std::size_t index = 0; index < size; ++index) {
                const auto value = event_value(space->asset_at(index), slope[index], intercept[index]);
                next_slope[index] = value[0];
                next_intercept[index] = value[1];
            }
            std::optional<std::size_t> previous;
            for (const double level : breaks) {
                const std::size_t index = space->nearest_node(level);
                if (previous == index) continue;
                previous = index;
                // Average only the event change, preserving smooth continuation between observations.
                const auto correction = project_finite_difference_event<2>(*space, breaks, index, [&](double asset) {
                    const double continuation_slope = space->linear_interpolate(slope, asset);
                    const double continuation_intercept = space->linear_interpolate(intercept, asset);
                    const auto value = event_value(asset, continuation_slope, continuation_intercept);
                    return std::array{value[0] - continuation_slope, value[1] - continuation_intercept};
                });
                next_slope[index] = slope[index] + correction[0];
                next_intercept[index] = intercept[index] + correction[1];
            }
            slope.swap(next_slope);
            if (has_future_purchases) intercept.swap(next_intercept);
            damping.restart();
        }
    }

    std::vector<double> slope_curvature, intercept_curvature;
    const auto price_at = [&](double shifted_spot) {
        return space->interpolate(slope, shifted_spot, slope_curvature) * initial.quantity +
               space->interpolate(intercept, shifted_spot, intercept_curvature);
    };
    const double value = price_at(spot);
    return finite_difference_result(value, spot, space->upper, output, spot_shift, [&](double shifted_spot) { return checked_price(price_at(shifted_spot)); }, [&](double shifted_spot) { return space->delta(slope, shifted_spot) * initial.quantity + space->delta(intercept, shifted_spot); }, [&](double shifted_spot) { return space->gamma(slope, shifted_spot) * initial.quantity + space->gamma(intercept, shifted_spot); });
}

Result<double> FiniteDifferenceAccumulatorEngine::price(
    const Accumulator& option, const PricingContext& context) const
{
    return detail::price_value(price_native(option, context, GreeksRequest{}));
}

Result<PricingResult> FiniteDifferenceAccumulatorEngine::price_with_greeks(const Accumulator& option, const PricingContext& context,
                                                                           GreeksRequest greeks, NumericalShiftSettings settings) const
{
    return detail::price_with_greeks(*this, option, context, greeks, settings,
                                     [&](const auto& engine, const PricingContext& shifted, GreeksRequest request) {
                                         return engine.price_native(option, shifted, request, settings.spot_shift);
                                     });
}

} // namespace kiyosi
