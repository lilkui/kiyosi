#include <kiyosi/pricing/engines/digital/finite_difference.hpp>

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
Result<PricingResult> price_digital_fd(const Option& option, const PricingContext& context,
                                       FiniteDifferenceSettings settings, bool asset, RiskMeasureOutput requested_output)
{
    const auto valid = validate_valuation_within_instrument_life(context.valuation_time(), option.effective_date(), option.expiry_date());
    if (!valid) return std::unexpected(valid.error());
    auto settings_valid = validate_finite_difference_settings(settings);
    if (!settings_valid) return std::unexpected(settings_valid.error());
    if (settings.asset_step_count > 10'000 || settings.time_step_count > 100'000)
        return std::unexpected(Error{ErrorCategory::invalid_parameter, "finite-difference grid dimensions are out of range"});

    const double time = actual_365_fixed_year_fraction(context.valuation_time(), option.expiry_date());
    const double spot = context.spot_price();
    const double strike = option.strike();
    const double sign = option.option_type() == OptionType::call ? 1.0 : -1.0;
    const double payout = [&] {
        if constexpr (requires { option.payout(); }) return option.payout();
        else return 1.0;
    }();
    if (time == 0.0) {
        const bool in_the_money = sign * (spot - strike) > 0.0;
        return make_pricing_result(
            {{RiskMeasure::price, in_the_money ? (asset ? spot : payout) : 0.0}});
    }

    const double rate = context.model_parameters().risk_free_rate();
    const double dividend = context.model_parameters().dividend_yield();
    const double volatility = context.model_parameters().volatility();
    const int asset_step_count = settings.asset_step_count;
    const auto space = make_spatial_grid(settings, std::max(4.0 * strike, 4.0 * spot), {spot, strike});
    if (!space) return std::unexpected(space.error());
    const double spacing = space->spacing;
    const int time_step_count = settings.time_step_count;
    const auto grid = make_finite_difference_time_grid(time, time_step_count);
    if (auto stable = check_explicit_stability(settings.scheme, grid, volatility, rate, asset_step_count);
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
        if (asset) return Boundaries{0.0, call ? space->upper * std::exp(-dividend * tau) : 0.0};
        const double discounted = payout * std::exp(-rate * tau);
        return call ? Boundaries{0.0, discounted} : Boundaries{discounted, 0.0};
    };

    std::vector<double> old(space->size());
    for (int index = 0; index <= asset_step_count; ++index)
        old[static_cast<std::size_t>(index)] = terminal(spacing * index);

    const auto marched = march_backward(grid, DiffusionParameters{rate, dividend, volatility, theta},
                                        old, boundary);
    if (!marched) return std::unexpected(marched.error());

    if (requested_output == RiskMeasureOutput::price_only)
        return make_pricing_result({{RiskMeasure::price, space->interpolate(old, spot)}});

    auto output = make_pricing_result(
        {{RiskMeasure::price, space->interpolate(old, spot)},
         {RiskMeasure::delta, space->delta(old, spot)},
         {RiskMeasure::gamma, space->gamma(old, spot)}});
    if (!output) return std::unexpected(output.error());
    if (!output->all_finite())
        return std::unexpected(Error{ErrorCategory::invalid_result, "finite-difference pricing produced a non-finite result"});
    return output;
}

} // namespace

Result<PricingResult> FiniteDifferenceDigitalEngine::price_cash_or_nothing(
    const CashOrNothingOption& option, const PricingContext& context, detail::RiskMeasureOutput output) const
{
    return price_digital_fd(option, context, settings_, false, output);
}
Result<PricingResult> FiniteDifferenceDigitalEngine::price_asset_or_nothing(
    const AssetOrNothingOption& option, const PricingContext& context, detail::RiskMeasureOutput output) const
{
    return price_digital_fd(option, context, settings_, true, output);
}

} // namespace kiyosi
