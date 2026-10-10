#include <kiyosi/pricing/engines/vanilla/finite_difference.hpp>

#include <kiyosi/pricing/numerical_greeks.hpp>

#include <algorithm>
#include <cmath>
#include <numbers>
#include <vector>

#include "../../detail/black_scholes.hpp"
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
    const auto space = make_spatial_grid(settings, default_finite_difference_upper_boundary(option, context), {spot, strike});
    if (!space) return std::unexpected(space.error());
    const double upper = space->upper;
    // S = strike * sinh(x) keeps the zero boundary and concentrates nodes near the strike.
    const auto coordinate_of = [&](double asset) {
        const double ratio = asset / strike;
        return std::isfinite(ratio) ? std::asinh(ratio) : log_price_ratio(asset, strike) + std::numbers::ln2;
    };
    const double coordinate_upper = american ? coordinate_of(upper) : upper;
    const double spacing = coordinate_upper / asset_step_count;
    const SpatialGrid coordinate{coordinate_upper, spacing, asset_step_count};
    const double coordinate_spot = american ? coordinate_of(spot) : spot;
    const auto asset = [&](int index) {
        if (!american) return spacing * index;
        const double x = spacing * index;
        return x > 700.0 ? std::exp(std::log(strike) + x - std::numbers::ln2) : strike * std::sinh(x);
    };
    const int time_step_count = settings.time_step_count;
    const auto grid = make_finite_difference_time_grid(time, time_step_count);
    const double theta = scheme_theta(settings.scheme);
    const DiffusionParameters parameters{rate, dividend, volatility, theta, american ? spacing : 0.0};
    if (american && settings.scheme == FiniteDifferenceScheme::explicit_euler) {
        double maximum_decay = 0.0;
        for (int index = 1; index < asset_step_count; ++index)
            maximum_decay = std::max(maximum_decay, -diffusion_coefficients(index, parameters)[1]);
        if (time / time_step_count * maximum_decay > 1.0)
            return std::unexpected(Error{ErrorCategory::invalid_parameter, "explicit finite-difference grid is unstable"});
    } else if (auto stable = check_explicit_stability(settings.scheme, grid, volatility, rate, dividend, asset_step_count);
               !stable) {
        return std::unexpected(stable.error());
    }

    auto boundary = [&](double tau) {
        const bool call = option.option_type() == OptionType::call;
        // The asymptotic edge loses option time value for long or volatile maturities.
        const auto probabilities = black_scholes_probabilities(sign, upper, strike, rate, dividend, volatility, tau);
        const double continuation = sign * (upper * std::exp(-dividend * tau) * probabilities.asset -
                                            strike * std::exp(-rate * tau) * probabilities.cash);
        const double high = american ? std::max({sign * (upper - strike), 0.0, continuation}) : continuation;
        const double discounted_strike = strike * std::exp(-rate * tau);
        const double low = call ? 0.0 : (american ? std::max(strike, discounted_strike) : discounted_strike);
        return Boundaries{low, high};
    };
    auto intrinsic = [&](double underlying) {
        return std::max(sign * (underlying - strike), 0.0);
    };

    std::vector<double> old(space->size());
    for (int index = 0; index <= asset_step_count; ++index)
        old[static_cast<std::size_t>(index)] = intrinsic(asset(index));
    const auto exercise = american ? old : std::vector<double>{};

    const auto marched = march_backward(
        grid, parameters, old, boundary,
        [&](std::vector<double>& layer, double, double) {
            if (!american) return;
            for (int index = 1; index < asset_step_count; ++index)
                layer[static_cast<std::size_t>(index)] =
                    std::max(layer[static_cast<std::size_t>(index)], exercise[static_cast<std::size_t>(index)]);
        });
    if (!marched) return std::unexpected(marched.error());

    const double value = std::max(coordinate.interpolate(old, coordinate_spot), american ? intrinsic(spot) : 0.0);
    const double derivative = requested_output.has(Greek::delta) || requested_output.has(Greek::gamma)
                                  ? coordinate.delta(old, coordinate_spot)
                                  : 0.0;
    const double jacobian = american ? std::hypot(strike, spot) : 1.0;
    const double gamma = requested_output.has(Greek::gamma)
                             ? (coordinate.gamma(old, coordinate_spot) -
                                (american ? derivative * std::tanh(coordinate_spot) : 0.0)) /
                                   jacobian / jacobian
                             : 0.0;
    return make_pricing_result(value, {{Greek::delta, requested_output.has(Greek::delta) ? std::optional{derivative / jacobian} : std::nullopt},
                                       {Greek::gamma, requested_output.has(Greek::gamma) ? std::optional{gamma} : std::nullopt}});
}
} // namespace

Result<PricingResult> FiniteDifferenceVanillaEngine::price_native(const EuropeanOption& option, const PricingContext& context, GreeksRequest output) const
{
    return price_finite_difference(option, context, settings_, false, output);
}

Result<PricingResult> FiniteDifferenceVanillaEngine::price_native(const AmericanOption& option, const PricingContext& context, GreeksRequest output) const
{
    return price_finite_difference(option, context, settings_, true, output);
}

Result<double> FiniteDifferenceVanillaEngine::price(const EuropeanOption& option, const PricingContext& context) const
{
    return detail::price_value(price_native(option, context, GreeksRequest{}));
}

Result<double> FiniteDifferenceVanillaEngine::price(const AmericanOption& option, const PricingContext& context) const
{
    return detail::price_value(price_native(option, context, GreeksRequest{}));
}

Result<PricingResult> FiniteDifferenceVanillaEngine::price_with_greeks(const EuropeanOption& option, const PricingContext& context,
                                                                       GreeksRequest greeks, NumericalShiftSettings settings) const
{
    return detail::price_with_greeks(*this, option, context, greeks, settings, [&](const auto& engine, const PricingContext& shifted, GreeksRequest request) { return engine.price_native(option, shifted, request); });
}

Result<PricingResult> FiniteDifferenceVanillaEngine::price_with_greeks(const AmericanOption& option, const PricingContext& context,
                                                                       GreeksRequest greeks, NumericalShiftSettings settings) const
{
    return detail::price_with_greeks(*this, option, context, greeks, settings, [&](const auto& engine, const PricingContext& shifted, GreeksRequest request) { return engine.price_native(option, shifted, request); });
}

} // namespace kiyosi
