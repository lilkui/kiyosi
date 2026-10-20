#include <kiyosi/pricing/engines/vanilla/finite_difference.hpp>

#include <kiyosi/pricing/detail/numerical_greeks.hpp>

#include <algorithm>
#include <cmath>
#include <concepts>
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
    FiniteDifferenceSettings settings, GreeksRequest requested_output, double spot_shift = NumericalShiftSettings{}.spot_shift)
{
    constexpr bool american = std::same_as<Option, AmericanOption>;
    const auto valid_expiry = validate_valuation_within_instrument_life(context.valuation_time(), option.effective_date(), option.expiry_date());
    if (!valid_expiry) return std::unexpected(valid_expiry.error());

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
    const auto space = make_spatial_grid(settings, default_finite_difference_upper_boundary(option, context), {spot, strike}, american ? strike : 0.0);
    if (!space) return std::unexpected(space.error());
    const double upper = space->upper;
    const int time_step_count = settings.time_step_count;
    const auto grid = make_finite_difference_time_grid(time, time_step_count);
    const double theta = scheme_theta(settings.scheme);
    const DiffusionParameters parameters{rate, dividend, volatility, theta, american ? space->spacing : 0.0};
    if (auto stable = check_explicit_stability(settings.scheme, grid, parameters, asset_step_count); !stable) {
        return std::unexpected(stable.error());
    }

    auto boundary = [&](double tau) {
        const bool call = option.option_type() == OptionType::call;
        // The asymptotic edge loses option time value for long or volatile maturities.
        const auto probabilities = black_scholes_probabilities(sign, upper, strike, rate, dividend, volatility, tau);
        const double continuation = black_scholes_values(sign, upper, strike, -dividend * tau,
                                                         -rate * tau, probabilities)
                                        .price;
        const double high = american ? std::max({sign * (upper - strike), 0.0, continuation}) : continuation;
        const double discounted_strike = scaled_exponential(strike, -rate * tau);
        const double low = call ? 0.0 : (american ? std::max(strike, discounted_strike) : discounted_strike);
        return Boundaries{low, high};
    };
    auto intrinsic = [&](double underlying) {
        return std::max(sign * (underlying - strike), 0.0);
    };

    std::vector<double> old(space->size());
    for (std::size_t index = 0; index < old.size(); ++index)
        old[index] = intrinsic(space->asset_at(index));
    const auto exercise = american ? old : std::vector<double>{};

    const auto marched = march_backward<true>(
        grid, parameters, old, boundary,
        [&](std::vector<double>& layer, double, double) {
            if (!american) return;
            for (int index = 1; index < asset_step_count; ++index)
                layer[static_cast<std::size_t>(index)] =
                    std::max(layer[static_cast<std::size_t>(index)], exercise[static_cast<std::size_t>(index)]);
        });
    if (!marched) return std::unexpected(marched.error());

    std::vector<double> curvature;
    const double value = std::max(space->interpolate(old, spot, curvature), american ? intrinsic(spot) : 0.0);
    return finite_difference_result(value, spot, upper, requested_output, spot_shift, [&](double shifted_spot) { return checked_price(std::max(space->interpolate(old, shifted_spot, curvature), american ? intrinsic(shifted_spot) : 0.0)); }, [&](double shifted_spot) { return space->delta(old, shifted_spot); }, [&](double shifted_spot) { return space->gamma(old, shifted_spot); });
}
} // namespace

Result<double> FiniteDifferenceVanillaEngine::price(const EuropeanOption& option, const PricingContext& context) const
{
    return detail::price_value(price_finite_difference(option, context, settings_, GreeksRequest{}));
}

Result<double> FiniteDifferenceVanillaEngine::price(const AmericanOption& option, const PricingContext& context) const
{
    return detail::price_value(price_finite_difference(option, context, settings_, GreeksRequest{}));
}

Result<PricingResult> FiniteDifferenceVanillaEngine::price_with_greeks(const EuropeanOption& option, const PricingContext& context,
                                                                       GreeksRequest greeks, NumericalShiftSettings settings) const
{
    return detail::price_with_greeks(*this, option, context, greeks, settings, [&](const auto& engine, const PricingContext& shifted, GreeksRequest request) { return price_finite_difference(option, shifted, engine.settings(), request, settings.spot_shift); });
}

Result<PricingResult> FiniteDifferenceVanillaEngine::price_with_greeks(const AmericanOption& option, const PricingContext& context,
                                                                       GreeksRequest greeks, NumericalShiftSettings settings) const
{
    return detail::price_with_greeks(*this, option, context, greeks, settings, [&](const auto& engine, const PricingContext& shifted, GreeksRequest request) { return price_finite_difference(option, shifted, engine.settings(), request, settings.spot_shift); });
}

} // namespace kiyosi
