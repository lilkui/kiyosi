#include <kiyosi/pricing/engines/accumulator/finite_difference.hpp>

#include <kiyosi/pricing/numerical_greeks.hpp>

#include <algorithm>
#include <cmath>
#include <vector>

#include "../../detail/accumulator_state.hpp"
#include "../../detail/calendar_dates.hpp"
#include "../../detail/fd_grid.hpp"
#include "../../detail/fd_scheme.hpp"
#include "../../detail/math.hpp"

namespace kiyosi {
using namespace detail;

namespace {

/// Value at expiry_date per unit of the affine decomposition: `slope` multiplies the quantity already
/// accumulated, `intercept` holds the value of the quantity still to be bought.
void seed_expiry_layers(const Accumulator& option, const SpatialGrid& space, std::vector<double>& slope,
                        std::vector<double>& intercept)
{
    for (std::size_t index = 0; index < space.size(); ++index) {
        const double asset = space.spacing * static_cast<double>(index);
        slope[index] = asset - option.strike();
        if (asset >= option.knock_out_level()) {
            intercept[index] = 0.0;
            continue;
        }
        const double bought = asset < option.strike()
                                  ? option.daily_quantity() * option.acceleration_factor()
                                  : option.daily_quantity();
        intercept[index] = slope[index] * bought;
    }
}

} // namespace

Result<PricingResult> FiniteDifferenceAccumulatorEngine::price_native(
    const Accumulator& option, const PricingContext& context, GreeksRequest output) const
{
    auto valid = validate_valuation_within_instrument_life(context.valuation_time(), option.effective_date(), option.expiry_date());
    if (!valid) return std::unexpected(valid.error());
    auto expiry_valid = validate_trading_expiry(context.calendar(), option.expiry_date());
    if (!expiry_valid) return std::unexpected(expiry_valid.error());
    auto settings_valid = detail::validate_finite_difference_settings(
        settings_, trading_fd_max_steps, trading_fd_max_steps);
    if (!settings_valid) return std::unexpected(settings_valid.error());

    const auto initial = accumulator_initial_state(option, context);
    if (initial.settlement) return make_pricing_result(*initial.settlement);

    const double spot = context.spot_price();
    const double time_to_expiry = actual_365_fixed_year_fraction(context.valuation_time(), option.expiry_date());

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
    const auto grid = make_finite_difference_time_grid(time_to_expiry, settings_.time_step_count, trading_times);
    if (auto stable = check_explicit_stability(settings_.scheme, grid, sigma, rate, dividend, settings_.asset_step_count);
        !stable)
        return std::unexpected(stable.error());

    const std::size_t size = space->size();
    std::vector<double> slope(size), intercept(size), next_slope(size), next_intercept(size);
    seed_expiry_layers(option, *space, slope, intercept);

    LinearBoundaryStepper stepper(
        size, space->upper, space->spacing,
        DiffusionParameters{rate, dividend, sigma, scheme_theta(settings_.scheme)});
    for (std::size_t step = grid.size() - 1; step-- > 0;) {
        const double dt = grid[step + 1] - grid[step];
        if (!stepper.advance_pair(slope, next_slope, intercept, next_intercept, dt))
            return std::unexpected(Error{ErrorCategory::invalid_result,
                                         "finite-difference system is numerically unstable"});
        if (std::ranges::binary_search(trading_times, grid[step])) {
            for (std::size_t index = 0; index < size; ++index) {
                const double asset = space->spacing * static_cast<double>(index);
                if (asset >= option.knock_out_level()) {
                    // Knock-out settles every unit accumulated up to this event immediately.
                    // The affine value is quantity * (asset - strike), with no future purchases.
                    next_slope[index] = asset - option.strike();
                    next_intercept[index] = 0.0;
                } else {
                    next_intercept[index] +=
                        next_slope[index] * (asset < option.strike()
                                                 ? option.daily_quantity() * option.acceleration_factor()
                                                 : option.daily_quantity());
                }
            }
        }
        slope.swap(next_slope);
        intercept.swap(next_intercept);
    }

    return make_pricing_result(space->interpolate(slope, spot) * initial.quantity +
                                   space->interpolate(intercept, spot),
                               {{Greek::delta, output.has(Greek::delta) ? std::optional{space->delta(slope, spot) * initial.quantity + space->delta(intercept, spot)} : std::nullopt},
                                {Greek::gamma, output.has(Greek::gamma) ? std::optional{space->gamma(slope, spot) * initial.quantity + space->gamma(intercept, spot)} : std::nullopt}});
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
                                         const auto output = detail::at_spot_discontinuity(option, shifted, detail::symmetric_shift(shifted.spot_price(), settings.spot_shift)) ? GreeksRequest{} : request;
                                         return engine.price_native(option, shifted, output);
                                     });
}

} // namespace kiyosi
