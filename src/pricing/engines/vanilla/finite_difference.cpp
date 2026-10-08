#include <kiyosi/pricing/engines/vanilla/finite_difference.hpp>

#include <algorithm>
#include <cmath>
#include <vector>

#include "../../detail/fd_grid.hpp"
#include "../../detail/fd_scheme.hpp"
#include "../../detail/math.hpp"

namespace kiyosi {
using namespace detail;

namespace {
template <typename Option>
Result<PricingResult> price_finite_difference(
    const Option& option, const PricingContext& context,
    FiniteDifferenceSettings settings, bool american, GreeksRequest requested_output)
{
    const auto valid_expiry = validate_valuation_within_instrument_life(context.valuation_time(), option.effective_date(), option.expiry_date());
    if (!valid_expiry) return std::unexpected(valid_expiry.error());
    auto settings_valid = detail::validate_finite_difference_settings(
        settings, general_fd_max_asset_steps, general_fd_max_time_steps);
    if (!settings_valid) return std::unexpected(settings_valid.error());

    const double time = actual_365_fixed_year_fraction(context.valuation_time(), option.expiry_date());
    const double spot = context.spot_price();
    const double strike = option.strike();
    const double sign = option.option_type() == OptionType::call ? 1.0 : -1.0;
    if (time == 0.0) {
        return make_pricing_result(std::max(sign * (spot - strike), 0.0));
    }

    const double rate = context.model_parameters().risk_free_rate();
    const double dividend = context.model_parameters().dividend_yield();
    const double volatility = context.model_parameters().volatility();
    const int asset_step_count = settings.asset_step_count;
    const auto space = make_spatial_grid(settings, std::max(4.0 * strike, 4.0 * spot), {spot, strike});
    if (!space) return std::unexpected(space.error());
    const double upper = space->upper;
    const double spacing = space->spacing;
    const int time_step_count = settings.time_step_count;
    const auto grid = make_finite_difference_time_grid(time, time_step_count);
    if (auto stable = check_explicit_stability(settings.scheme, grid, volatility, rate, asset_step_count);
        !stable)
        return std::unexpected(stable.error());
    const double theta = scheme_theta(settings.scheme);

    auto boundary = [&](double tau) {
        const bool call = option.option_type() == OptionType::call;
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
    for (int index = 0; index <= asset_step_count; ++index)
        old[static_cast<std::size_t>(index)] = intrinsic(spacing * index);

    const auto marched = march_backward(
        grid, DiffusionParameters{rate, dividend, volatility, theta}, old, boundary,
        [&](std::vector<double>& layer, double, double) {
            if (!american) return;
            for (int index = 1; index < asset_step_count; ++index)
                layer[static_cast<std::size_t>(index)] =
                    std::max(layer[static_cast<std::size_t>(index)], intrinsic(spacing * index));
        });
    if (!marched) return std::unexpected(marched.error());

    return make_pricing_result(space->interpolate(old, spot), {{Greek::delta, requested_output.has(Greek::delta) ? std::optional{space->delta(old, spot)} : std::nullopt},
                                                                      {Greek::gamma, requested_output.has(Greek::gamma) ? std::optional{space->gamma(old, spot)} : std::nullopt}});
}
} // namespace

Result<PricingResult> FiniteDifferenceVanillaEngine::price_native(
    const EuropeanOption& option, const PricingContext& context, GreeksRequest output) const
{
    return price_finite_difference(option, context, settings_, false, output);
}

Result<PricingResult> FiniteDifferenceVanillaEngine::price_native(
    const AmericanOption& option, const PricingContext& context, GreeksRequest output) const
{
    return price_finite_difference(option, context, settings_, true, output);
}

} // namespace kiyosi
