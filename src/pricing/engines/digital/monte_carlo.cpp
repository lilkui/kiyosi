#include <kiyosi/pricing/engines/digital/monte_carlo.hpp>

#include <array>
#include <cmath>
#include <optional>
#include <random>
#include <type_traits>

#include "../../detail/math.hpp"
#include "../../detail/numerical_greeks.hpp"
#include "../monte_carlo_digital.hpp"
#include "../monte_carlo_host.hpp"

namespace kiyosi {
namespace {

template <bool delta, bool gamma>
Result<std::array<double, 3>> terminal_means(
    detail::DigitalMonteCarloParameters parameters, MonteCarloSettings settings)
{
    constexpr std::size_t count = 1 + delta + gamma;
    const auto values = detail::cpu_path_means<count>(settings, [&](auto& generator) -> Result<std::array<double, count>> {
        std::normal_distribution<double> normal;
        const auto sample = parameters.sample(detail::draw_monte_carlo_normal(generator, normal), delta, gamma);
        std::array<double, count> output{sample.price};
        if constexpr (delta) output[1] = sample.delta;
        if constexpr (gamma) output[count - 1] = sample.gamma;
        for (const double value : output)
            if (!std::isfinite(value))
                return std::unexpected(Error{ErrorCategory::invalid_result, "digital Monte Carlo produced a non-finite sample"});
        return output;
    });
    if (!values) return std::unexpected(values.error());
    return std::array{(*values)[0], delta ? (*values)[1] : 0.0, gamma ? (*values)[count - 1] : 0.0};
}

template <typename Option>
Result<PricingResult> price_digital(const Option& option, const PricingContext& context,
                                    MonteCarloSettings settings, GreeksRequest greeks)
{
    constexpr bool asset = std::same_as<Option, AssetOrNothingOption>;
    const double payout = [&] {
        if constexpr (asset) return 0.0;
        else return option.payout();
    }();
    const auto time = detail::time_to_expiry(context.valuation_time(), option.effective_date(), option.expiry_date());
    if (!time) return std::unexpected(time.error());
    const double spot = context.spot_price();
    if (*time == 0.0)
        return make_pricing_result(detail::binary_payoff_value(option.option_type(), option.strike(), payout, asset, spot));

    const auto& model = context.model_parameters();
    const double sigma = model.volatility();
    const double root_time = std::sqrt(*time);
    const double width = sigma * root_time;
    const double forward = detail::log_price_ratio(spot, option.strike()) +
                           ((*model.risk_free_rate()) - (*model.dividend_yield())) * *time;
    const double payoff_drift = asset ? (-(*model.dividend_yield()) - 0.5 * sigma * sigma) * *time
                                      : -(*model.risk_free_rate()) * *time;
    if (!std::isfinite(width) || !std::isfinite(forward) || !std::isfinite(payoff_drift))
        return std::unexpected(Error{ErrorCategory::invalid_result, "digital Monte Carlo parameters are non-finite"});
    const detail::DigitalMonteCarloParameters parameters{
        detail::standardize_forward(forward, sigma, root_time) - 0.5 * width,
        width, asset ? spot : payout, std::log(asset ? spot : payout), payoff_drift,
        std::log(spot), std::log(sigma) + 0.5 * std::log(*time), asset, option.option_type() == OptionType::call};
    const bool delta = greeks.has(Greek::delta), gamma = greeks.has(Greek::gamma);
    std::array<double, 3> means{};
    if (settings.backend == MonteCarloBackend::cuda) {
#if KIYOSI_HAS_CUDA
        const auto result = detail::cuda_digital_prices({settings.path_count,
                                                         settings.seed ? *settings.seed : detail::random_seed(), parameters, settings.antithetic, delta, gamma});
        const auto valid = detail::cuda_mean(result.result);
        if (!valid) return std::unexpected(valid.error());
        means = {result.payoff_means[0], result.payoff_means[1], result.payoff_means[2]};
#else
        return std::unexpected(Error{ErrorCategory::backend_unavailable, "CUDA support is not enabled in this build"});
#endif
    } else {
        const auto values = delta ? (gamma ? terminal_means<true, true>(parameters, settings)
                                           : terminal_means<true, false>(parameters, settings))
                                  : (gamma ? terminal_means<false, true>(parameters, settings)
                                           : terminal_means<false, false>(parameters, settings));
        if (!values) return std::unexpected(values.error());
        means = *values;
    }
    return make_pricing_result(means[0], {{Greek::delta, delta ? std::optional{means[1]} : std::nullopt},
                                          {Greek::gamma, gamma ? std::optional{means[2]} : std::nullopt}});
}

} // namespace

Result<double> MonteCarloDigitalEngine::price(const CashOrNothingOption& option, const PricingContext& context) const
{
    if (auto valid = detail::require_flat_rates(context); !valid) return std::unexpected(valid.error());
    return detail::price_value(price_digital(option, context, settings_, {}));
}

Result<double> MonteCarloDigitalEngine::price(const AssetOrNothingOption& option, const PricingContext& context) const
{
    if (auto valid = detail::require_flat_rates(context); !valid) return std::unexpected(valid.error());
    return detail::price_value(price_digital(option, context, settings_, {}));
}

Result<PricingResult> MonteCarloDigitalEngine::price_with_greeks(
    const CashOrNothingOption& option, const PricingContext& context, GreeksRequest greeks, NumericalShiftSettings settings) const
{
    if (auto valid = detail::require_flat_rates(context); !valid) return std::unexpected(valid.error());
    return detail::evaluate_greeks(*this, option, context, greeks, settings, [&](const auto& engine) {
        return price_digital(option, context, engine.settings(), greeks);
    });
}

Result<PricingResult> MonteCarloDigitalEngine::price_with_greeks(
    const AssetOrNothingOption& option, const PricingContext& context, GreeksRequest greeks, NumericalShiftSettings settings) const
{
    if (auto valid = detail::require_flat_rates(context); !valid) return std::unexpected(valid.error());
    return detail::evaluate_greeks(*this, option, context, greeks, settings, [&](const auto& engine) {
        return price_digital(option, context, engine.settings(), greeks);
    });
}

} // namespace kiyosi
