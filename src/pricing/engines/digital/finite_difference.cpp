#include <kiyosi/pricing/engines/digital/finite_difference.hpp>

#include <kiyosi/pricing/detail/numerical_greeks.hpp>

#include <algorithm>
#include <cmath>
#include <concepts>
#include <limits>
#include <vector>

#include "../../detail/black_scholes.hpp"
#include "../../detail/fd_grid.hpp"
#include "../../detail/fd_scheme.hpp"
#include "../../detail/math.hpp"

namespace kiyosi {
using namespace detail;

namespace {
template <typename Option>
Result<PricingResult> price_digital_fd(const Option& option, const PricingContext& context,
                                       FiniteDifferenceSettings settings, GreeksRequest requested_output, double spot_shift = NumericalShiftSettings{}.spot_shift)
{
    constexpr bool asset = std::same_as<Option, AssetOrNothingOption>;
    const auto valid = validate_valuation_within_instrument_life(context.valuation_time(), option.effective_date(), option.expiry_date());
    if (!valid) return std::unexpected(valid.error());

    const double time = actual_365_fixed_year_fraction(context.valuation_time(), option.expiry_date());
    const double spot = context.spot_price();
    const double strike = option.strike();
    const double sign = option.option_type() == OptionType::call ? 1.0 : -1.0;
    const double payout = [&] {
        if constexpr (requires { option.payout(); }) return option.payout();
        else return 1.0;
    }();
    if (time == 0.0)
        return make_pricing_result(binary_payoff_value(option.option_type(), strike, payout, asset, spot));

    const double rate = context.model_parameters().risk_free_rate();
    const double dividend = context.model_parameters().dividend_yield();
    const double volatility = context.model_parameters().volatility();
    const int asset_step_count = settings.asset_step_count;
    const auto space = make_spatial_grid(settings, default_finite_difference_upper_boundary(option, context), {spot, strike});
    if (!space) return std::unexpected(space.error());
    const double spacing = space->spacing;
    const int time_step_count = settings.time_step_count;
    const auto grid = make_finite_difference_time_grid(time, time_step_count);
    if (auto stable = check_explicit_stability(settings.scheme, grid, volatility, rate, dividend, asset_step_count);
        !stable)
        return std::unexpected(stable.error());
    const double theta = scheme_theta(settings.scheme);
    auto terminal = [&](double underlying) {
        // Average the discontinuous payoff over each cell to avoid strike-alignment bias.
        const double low = underlying - 0.5 * spacing;
        const double high = underlying + 0.5 * spacing;
        const double left = sign > 0.0 ? std::max(low, strike) : low;
        const double right = sign > 0.0 ? high : std::min(high, strike);
        if (left >= right) return 0.0;
        return (right - left) / spacing * (asset ? 0.5 * (left + right) : payout);
    };
    auto boundary = [&](double tau) {
        const bool call = option.option_type() == OptionType::call;
        const auto probabilities = black_scholes_probabilities(sign, space->upper, strike, rate, dividend, volatility, tau);
        const double amount = asset ? space->upper : payout;
        const double exponent = -(asset ? dividend : rate) * tau;
        const double discounted = scaled_exponential(amount, exponent);
        const double probability = asset ? probabilities.asset : probabilities.cash;
        const double upper = tau > 0.0 && volatility > 0.0 && (probability == 0.0 || !std::isfinite(discounted))
                                 ? scaled_normal_cdf(amount, exponent, sign * (asset ? probabilities.d1 : probabilities.d2))
                                 : discounted * probability;
        return Boundaries{asset || call ? 0.0 : discounted, upper};
    };

    std::vector<double> old(space->size());
    for (std::size_t index = 0; index < old.size(); ++index)
        old[index] = terminal(spacing * static_cast<double>(index));

    const auto marched = march_backward<true>(grid, DiffusionParameters{rate, dividend, volatility, theta},
                                              old, boundary);
    if (!marched) return std::unexpected(marched.error());

    const double carry = asset ? dividend : rate;
    const double relative_carry_error = [&] {
        if (theta != 0.5) return 0.0;
        RannacherDamping damping{2};
        double log_error = 0.0;
        for (std::size_t step = grid.time_steps.size(); step-- > 0;) {
            const auto substeps = damping.next(theta);
            // CN's scalar factor is no larger than exact for nonnegative carry.
            if (substeps.theta == 0.5 && carry >= 0.0) continue;
            const double exponent = carry * (grid.time_steps[step] / substeps.count);
            const double implicit = substeps.theta * exponent;
            const double explicit_part = -(1.0 - substeps.theta) * exponent;
            if (!std::isfinite(exponent) || implicit <= -1.0 || explicit_part <= -1.0) return 0.0;
            log_error += substeps.count * std::max(0.0, exponent + std::log1p(explicit_part) - std::log1p(implicit));
        }
        const double error = std::expm1(log_error);
        return std::isfinite(error) ? error : 0.0;
    }();
    std::vector<double> curvature;
    const auto price_at = [&](double shifted_spot) -> Result<double> {
        const double value = space->interpolate(old, shifted_spot, curvature);
        const double upper_bound = scaled_exponential(asset ? shifted_spot : payout, -(asset ? dividend : rate) * time);
        // Separate known scalar carry truncation from oscillations; negative prices allow only roundoff.
        const double tolerance = 64.0 * std::numeric_limits<double>::epsilon() * upper_bound;
        const double carry_error = upper_bound * relative_carry_error;
        const double upper_tolerance = tolerance + (std::isfinite(carry_error) ? carry_error : 0.0);
        if (!std::isfinite(value) || value < -tolerance || value - upper_bound > upper_tolerance)
            return std::unexpected(Error{ErrorCategory::invalid_result,
                                         "finite-difference digital price violates payoff bounds"});
        return std::clamp(value, 0.0, upper_bound);
    };
    const auto value = price_at(spot);
    if (!value) return std::unexpected(value.error());
    return finite_difference_result(*value, spot, space->upper, requested_output, spot_shift, price_at, [&](double shifted_spot) { return space->delta(old, shifted_spot); }, [&](double shifted_spot) { return space->gamma(old, shifted_spot); });
}

} // namespace

Result<double> FiniteDifferenceDigitalEngine::price(const CashOrNothingOption& option, const PricingContext& context) const
{
    return detail::price_value(price_digital_fd(option, context, settings_, GreeksRequest{}));
}

Result<double> FiniteDifferenceDigitalEngine::price(const AssetOrNothingOption& option, const PricingContext& context) const
{
    return detail::price_value(price_digital_fd(option, context, settings_, GreeksRequest{}));
}

Result<PricingResult> FiniteDifferenceDigitalEngine::price_with_greeks(const CashOrNothingOption& option, const PricingContext& context,
                                                                       GreeksRequest greeks, NumericalShiftSettings settings) const
{
    return detail::price_with_greeks(*this, option, context, greeks, settings, [&](const auto& engine, const PricingContext& shifted, GreeksRequest request) { return price_digital_fd(option, shifted, engine.settings(), request, settings.spot_shift); });
}

Result<PricingResult> FiniteDifferenceDigitalEngine::price_with_greeks(const AssetOrNothingOption& option, const PricingContext& context,
                                                                       GreeksRequest greeks, NumericalShiftSettings settings) const
{
    return detail::price_with_greeks(*this, option, context, greeks, settings, [&](const auto& engine, const PricingContext& shifted, GreeksRequest request) { return price_digital_fd(option, shifted, engine.settings(), request, settings.spot_shift); });
}

} // namespace kiyosi
