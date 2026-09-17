#include <kiyosi/pricing/engines/vanilla/finite_difference.hpp>

#include <algorithm>
#include <cmath>
#include <vector>

#include "../../detail/fd_grid.hpp"
#include "../../detail/fd_scheme.hpp"
#include "../../detail/math.hpp"

namespace kiyosi {
using namespace detail;

template <typename Option>
result<PricingResult> price_finite_difference(
    const Option& option, const PricingContext& context,
    FiniteDifferenceSettings settings, bool american)
{
    const auto valid_expiry = validate_life(context.valuation_time(), option.effective(), option.expiry());
    if (!valid_expiry) return std::unexpected(valid_expiry.error());
    auto settings_valid = validate_finite_difference_settings(settings);
    if (!settings_valid) return std::unexpected(settings_valid.error());
    if (settings.asset_steps > 10'000 || settings.time_steps > 100'000)
        return std::unexpected(Error{error_category::invalid_parameter, "finite-difference grid dimensions are out of range"});

    const double time = actual_365(context.valuation_time(), option.expiry());
    const double spot = context.asset_price();
    const double strike = option.strike();
    const double sign = option.type() == option_type::call ? 1.0 : -1.0;
    if (time == 0.0) {
        return make_pricing_result(
            {{risk_measure::price, std::max(sign * (spot - strike), 0.0)}});
    }

    const double rate = context.parameters().risk_free_rate();
    const double dividend = context.parameters().dividend_yield();
    const double volatility = context.parameters().volatility();
    const int asset_steps = settings.asset_steps;
    const auto space = make_spatial_grid(settings, std::max(4.0 * strike, 4.0 * spot), {spot, strike});
    if (!space) return std::unexpected(space.error());
    const double upper = space->upper;
    const double spacing = space->spacing;
    const int time_steps = settings.time_steps;
    const auto grid = finite_difference_grid(time, time_steps);
    if (auto stable = check_explicit_stability(settings.scheme, grid, volatility, rate, asset_steps);
        !stable)
        return std::unexpected(stable.error());
    const double theta = scheme_theta(settings.scheme);

    auto boundary = [&](double tau) {
        const bool call = option.type() == option_type::call;
        const double high = call ? (american ? upper - strike
                                             : upper * std::exp(-dividend * tau) - strike * std::exp(-rate * tau))
                                 : 0.0;
        const double low = call ? 0.0 : (american ? strike : strike * std::exp(-rate * tau));
        return Boundaries{low, high};
    };
    auto intrinsic = [&](double underlying) {
        return std::max(sign * (underlying - strike), 0.0);
    };

    std::vector<double> old(space->size());
    for (int index = 0; index <= asset_steps; ++index)
        old[static_cast<std::size_t>(index)] = intrinsic(spacing * index);

    const auto marched = march_backward(
        grid, DiffusionParameters{rate, dividend, volatility, theta}, old, boundary,
        [&](std::vector<double>& layer, double, double) {
            if (!american) return;
            for (int index = 1; index < asset_steps; ++index)
                layer[static_cast<std::size_t>(index)] =
                    std::max(layer[static_cast<std::size_t>(index)], intrinsic(spacing * index));
        });
    if (!marched) return std::unexpected(marched.error());

    auto output = make_pricing_result(
        {{risk_measure::price, space->interpolate(old, spot)},
         {risk_measure::delta, space->delta(old, spot)},
         {risk_measure::gamma, space->gamma(old, spot)}});
    if (!output) return std::unexpected(output.error());
    if (!output->all_finite())
        return std::unexpected(Error{error_category::invalid_result,
                                     "finite-difference pricing produced a non-finite result"});
    return output;
}

result<PricingResult> FiniteDifferenceVanillaEngine::price_european(
    const EuropeanOption& option, const PricingContext& context) const
{
    return price_finite_difference(option, context, settings_, false);
}

result<PricingResult> FiniteDifferenceVanillaEngine::price_american(
    const AmericanOption& option, const PricingContext& context) const
{
    return price_finite_difference(option, context, settings_, true);
}

} // namespace kiyosi
