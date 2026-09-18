#include <kiyosi/pricing/engines/accumulator/finite_difference.hpp>

#include <algorithm>
#include <cmath>
#include <vector>

#include "../../detail/calendar_dates.hpp"
#include "../../detail/fd_grid.hpp"
#include "../../detail/fd_scheme.hpp"
#include "../../detail/math.hpp"

namespace kiyosi {
using namespace detail;

namespace {

/// Value at expiry per unit of the affine decomposition: `slope` multiplies the quantity already
/// accumulated, `intercept` holds the value of the quantity still to be bought.
void seed_expiry_layers(const Accumulator& option, const PricingContext& context,
                        const SpatialGrid& space, std::vector<double>& slope,
                        std::vector<double>& intercept)
{
    const bool expiry_trading = context.calendar().is_trading_day(option.expiry());
    for (std::size_t index = 0; index < space.size(); ++index) {
        const double asset = space.spacing * static_cast<double>(index);
        slope[index] = asset - option.strike();
        if (!expiry_trading || asset >= option.knock_out()) {
            intercept[index] = 0.0;
            continue;
        }
        const double bought = asset < option.strike()
                                  ? option.daily_quantity() * option.acceleration()
                                  : option.daily_quantity();
        intercept[index] = slope[index] * bought;
    }
}

result<PricingResult> terminal_value(const Accumulator& option, const PricingContext& context)
{
    double quantity = option.accumulated_quantity();
    const double value = context.asset_price();
    if (context.calendar().is_trading_day(option.expiry()) && value < option.knock_out())
        quantity += value < option.strike() ? option.daily_quantity() * option.acceleration()
                                            : option.daily_quantity();
    return make_pricing_result(
        {{risk_measure::price, quantity * (value - option.strike())}});
}

} // namespace

result<PricingResult> FiniteDifferenceAccumulatorEngine::price(
    const Accumulator& option, const PricingContext& context) const
{
    auto valid = validate_life(context.valuation_time(), option.effective(), option.expiry());
    if (!valid) return std::unexpected(valid.error());
    auto settings_valid = validate_finite_difference_settings(settings_);
    if (!settings_valid) return std::unexpected(settings_valid.error());
    if (settings_.asset_steps > 2000 || settings_.time_steps > 2000)
        return std::unexpected(Error{error_category::invalid_parameter,
                                     "finite-difference grid dimensions are out of range"});
    auto contract = make_accumulator({option.strike(), option.knock_out(), option.daily_quantity(),
                                      option.acceleration(), option.accumulated_quantity(),
                                      option.effective(), option.expiry()});
    if (!contract) return std::unexpected(contract.error());

    const double spot = context.asset_price();
    const double relevant = std::max({spot, option.strike(), option.knock_out()});
    const auto space = make_spatial_grid(settings_, std::max(4.0 * relevant, relevant + 1.0), {relevant});
    if (!space) return std::unexpected(space.error());

    const double maturity = actual_365(context.valuation_time(), option.expiry());
    if (maturity == 0.0) return terminal_value(option, context);

    const auto future_trading_dates =
        trading_dates(context.calendar(), context.valuation_time(), option.expiry(), true);
    std::vector<double> trading_times;
    std::vector<double> anchors{0.0, maturity};
    trading_times.reserve(future_trading_dates.size());
    for (const date value : future_trading_dates) {
        const double time = actual_365(context.valuation_time(), value);
        trading_times.push_back(time);
        if (time > 0.0 && time < maturity) anchors.push_back(time);
    }

    const double rate = context.parameters().risk_free_rate();
    const double dividend = context.parameters().dividend_yield();
    const double sigma = context.parameters().volatility();
    const auto grid = finite_difference_grid(maturity, settings_.time_steps, std::move(anchors));
    if (auto stable = check_explicit_stability(settings_.scheme, grid, sigma, rate, settings_.asset_steps);
        !stable)
        return std::unexpected(stable.error());

    const std::size_t size = space->size();
    std::vector<double> slope(size), intercept(size), next_slope(size), next_intercept(size);
    seed_expiry_layers(option, context, *space, slope, intercept);

    LinearBoundaryStepper stepper(
        size, space->upper, space->spacing,
        DiffusionParameters{rate, dividend, sigma, scheme_theta(settings_.scheme)});
    for (std::size_t step = grid.size() - 1; step-- > 0;) {
        const double dt = grid[step + 1] - grid[step];
        if (!stepper.advance_pair(slope, next_slope, intercept, next_intercept, dt))
            return std::unexpected(Error{error_category::invalid_result,
                                         "finite-difference system is numerically unstable"});
        if (std::binary_search(trading_times.begin(), trading_times.end(), grid[step])) {
            for (std::size_t index = 0; index < size; ++index) {
                const double asset = space->spacing * static_cast<double>(index);
                if (asset >= option.knock_out()) {
                    next_intercept[index] += next_slope[index] * option.accumulated_quantity();
                    next_slope[index] = 0.0;
                } else {
                    next_intercept[index] +=
                        next_slope[index] * (asset < option.strike()
                                                 ? option.daily_quantity() * option.acceleration()
                                                 : option.daily_quantity());
                }
            }
        }
        slope.swap(next_slope);
        intercept.swap(next_intercept);
    }

    return make_pricing_result(
        {{risk_measure::price,
          space->interpolate(slope, spot) * option.accumulated_quantity() +
              space->interpolate(intercept, spot)}});
}

} // namespace kiyosi
