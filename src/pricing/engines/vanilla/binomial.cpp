#include <kiyosi/pricing/engines/vanilla/binomial.hpp>

#include "../../detail/numerical_greeks.hpp"

#include <algorithm>
#include <array>
#include <cmath>
#include <concepts>
#include <optional>
#include <vector>

#include "../../detail/math.hpp"
#include "../../detail/binomial.hpp"
#include "../monte_carlo_mean.hpp"

namespace kiyosi {
using namespace detail;

namespace {

std::array<double, 3> european_level(std::span<const double> terminal, double probability,
                                     double discount, int level)
{
    const int steps = static_cast<int>(terminal.size()) - 1 - level;
    const double log_discount = steps == 0 ? 0.0 : steps * std::log(discount);
    std::array<double, 3> result{};
    if (steps == 0 || probability == 0.0 || probability == 1.0) {
        const auto first = probability == 1.0 ? static_cast<std::size_t>(steps) : 0;
        for (int node = 0; node <= level; ++node)
            result[node] = scaled_exponential(terminal[first + node], log_discount);
        return result;
    }
    const int mode = std::min(steps, static_cast<int>((steps + 1.0) * probability));
    const double log_odds = std::log(probability) - std::log1p(-probability);
    std::vector<double> log_weights(static_cast<std::size_t>(steps) + 1);
    // Relative weights start at the mode; neither endpoint probability needs to be representable.
    for (int node = mode; node > 0; --node)
        log_weights[node - 1] = log_weights[node] + std::log(static_cast<double>(node) / (steps - node + 1)) - log_odds;
    for (int node = mode; node < steps; ++node)
        log_weights[node + 1] = log_weights[node] + std::log(static_cast<double>(steps - node) / (node + 1)) + log_odds;
    MonteCarloMean normalization{};
    std::array<MonteCarloMean, 3> sums{};
    for (int index = 0; index <= steps; ++index) {
        const double weight = std::exp(log_weights[index]);
        normalization.add(weight);
        // Validated payoffs are finite; weights at most one cannot overflow their products.
        const bool direct_weight = std::isnormal(weight) && weight <= 1.0;
        for (int node = 0; node <= level; ++node)
            sums[node].add(direct_weight ? terminal[index + node] * weight : scaled_exponential(terminal[index + node], log_weights[index]));
    }
    const double total = std::ldexp(normalization.sum, normalization.exponent);
    for (int node = 0; node <= level; ++node)
        result[node] = scaled_exponential(std::ldexp(sums[node].sum / total, sums[node].exponent), log_discount);
    return result;
}

template <typename Option>
Result<PricingResult> price_binomial(
    const Option& option, const PricingContext& context, BinomialSettings settings, GreeksRequest requested_output)
{
    constexpr bool american = std::same_as<Option, AmericanOption>;
    const auto step = binomial_time_step(context, option.effective_date(), option.expiry_date(), settings);
    if (!step) return std::unexpected(step.error());

    const double spot = context.spot_price();
    const double strike = option.strike();
    const double sign = option.option_type() == OptionType::call ? 1.0 : -1.0;
    const double dt = *step;
    if (dt == 0.0) {
        return make_pricing_result(std::max(sign * (spot - strike), 0.0));
    }

    const double rate = context.model_parameters().risk_free_rate();
    const double dividend = context.model_parameters().dividend_yield();
    const double volatility = context.model_parameters().volatility();
    const double root_dt = std::sqrt(dt);
    const double log_up = volatility * root_dt;
    const double up = std::exp(log_up);
    const double down = 1.0 / up;
    const double discount = std::exp(-rate * dt);
    // Avoid subtracting nearly equal exponentials at the admissible volatility boundary.
    const double probability = std::expm1((rate - dividend) * dt + log_up) / std::expm1(2.0 * log_up);
    if (!std::isfinite(up) || !std::isfinite(down) || up == down || !std::isfinite(discount) ||
        !std::isfinite(probability) || probability < 0.0 || probability > 1.0) {
        return std::unexpected(Error{ErrorCategory::invalid_result,
                                     "binomial parameters produced an unstable tree"});
    }

    std::vector<double> values(static_cast<std::size_t>(settings.step_count) + 1);
    const double up_squared = up * up;
    double node_spot = spot * std::pow(down, settings.step_count);
    if (!std::isfinite(up_squared) || !std::isnormal(node_spot)) {
        return std::unexpected(Error{ErrorCategory::invalid_result,
                                     "binomial tree asset price overflowed or underflowed"});
    }
    for (int node = 0; node <= settings.step_count; ++node) {
        if (!std::isfinite(node_spot)) {
            return std::unexpected(Error{ErrorCategory::invalid_result,
                                         "binomial tree produced a non-finite asset price"});
        }
        values[static_cast<std::size_t>(node)] = std::max(sign * (node_spot - strike), 0.0);
        node_spot *= up_squared;
    }

    std::array<double, 3> level_two{};
    std::array<double, 2> level_one{};
    const bool no_early_exercise = !american || (option.option_type() == OptionType::call && dividend <= 0.0 && rate >= 0.0);
    // Keep rollback's intermediate-overflow rejection when the terminal bound is not representable.
    const bool direct = no_early_exercise && settings.step_count >= 512 && std::isfinite(scaled_exponential(std::max(values.front(), values.back()), settings.step_count * std::log(discount)));
    if (direct) {
        const double value = european_level(values, probability, discount, 0)[0];
        if (requested_output.has(Greek::gamma) && settings.step_count >= 2) {
            level_two = european_level(values, probability, discount, 2);
            level_one = {discount * (probability * level_two[1] + (1.0 - probability) * level_two[0]),
                         discount * (probability * level_two[2] + (1.0 - probability) * level_two[1])};
        } else if (requested_output.has(Greek::delta)) {
            const auto first = european_level(values, probability, discount, 1);
            level_one = {first[0], first[1]};
        }
        // Roundoff must not put an American call below immediate exercise.
        values[0] = std::max(value, american ? spot - strike : 0.0);
    } else {
        if (requested_output.has(Greek::delta) && settings.step_count == 1) level_one = {values[0], values[1]};
        if (requested_output.has(Greek::gamma) && settings.step_count == 2) level_two = {values[0], values[1], values[2]};
        node_spot = spot * std::pow(down, settings.step_count - 1);
        for (int level = settings.step_count - 1; level >= 0; --level) {
            if (level < settings.step_count - 1) node_spot *= up;
            if constexpr (american) {
                if (level == 0) {
                    const double continuation = discount * (probability * values[1] + (1.0 - probability) * values[0]);
                    const double exercise = sign * (spot - strike);
                    if (exercise > 0.0 && exercise > continuation)
                        return make_pricing_result(exercise,
                                                   {{Greek::delta, requested_output.has(Greek::delta) ? std::optional{sign} : std::nullopt},
                                                    {Greek::gamma, requested_output.has(Greek::gamma) ? std::optional{0.0} : std::nullopt}});
                }
            }
            if (!binomial_rollback<american>(std::span{values}.first(static_cast<std::size_t>(level) + 2), discount, probability,
                                             node_spot, up_squared, strike, sign)) {
                return std::unexpected(Error{ErrorCategory::invalid_result,
                                             "binomial pricing produced a non-finite result"});
            }
            if (requested_output.has(Greek::gamma) && level == 2) {
                level_two = {values[0], values[1], values[2]};
            } else if (requested_output.has(Greek::delta) && level == 1) {
                level_one = {values[0], values[1]};
            }
        }
    }

    if (!requested_output.has(Greek::delta) && !requested_output.has(Greek::gamma))
        return make_pricing_result(values[0]);

    double delta = 0.0;
    double gamma = 0.0;
    bool gamma_available = false;
    if (requested_output.has(Greek::delta)) {
        const double denominator = spot * (up - down);
        if (std::isfinite(denominator) && denominator != 0.0)
            delta = (level_one[1] - level_one[0]) / denominator;
    }
    if (requested_output.has(Greek::gamma) && settings.step_count >= 2) {
        const double delta_up_denominator = spot * (up * up - 1.0);
        const double delta_down_denominator = spot * (1.0 - down * down);
        const double gamma_denominator = 0.5 * spot * (up * up - down * down);
        if (delta_up_denominator != 0.0 && delta_down_denominator != 0.0 && gamma_denominator != 0.0) {
            const double delta_up = (level_two[2] - level_two[1]) / delta_up_denominator;
            const double delta_down = (level_two[1] - level_two[0]) / delta_down_denominator;
            gamma = (delta_up - delta_down) / gamma_denominator;
            gamma_available = std::isfinite(gamma);
        }
    }

    return make_pricing_result(values[0], {{Greek::delta, requested_output.has(Greek::delta) ? std::optional{delta} : std::nullopt},
                                           {Greek::gamma,
                                            gamma_available ? std::optional<double>{gamma} : std::nullopt}});
}
} // namespace

Result<double> CoxRossRubinsteinVanillaEngine::price(const EuropeanOption& option, const PricingContext& context) const
{
    return detail::price_value(price_binomial(option, context, settings_, GreeksRequest{}));
}

Result<double> CoxRossRubinsteinVanillaEngine::price(const AmericanOption& option, const PricingContext& context) const
{
    return detail::price_value(price_binomial(option, context, settings_, GreeksRequest{}));
}

Result<PricingResult> CoxRossRubinsteinVanillaEngine::price_with_greeks(const EuropeanOption& option, const PricingContext& context,
                                                                        GreeksRequest greeks, NumericalShiftSettings settings) const
{
    return detail::price_with_greeks(*this, option, context, greeks, settings, [&](const auto& engine, const PricingContext& shifted, GreeksRequest request) { return price_binomial(option, shifted, engine.settings(), request); });
}

Result<PricingResult> CoxRossRubinsteinVanillaEngine::price_with_greeks(const AmericanOption& option, const PricingContext& context,
                                                                        GreeksRequest greeks, NumericalShiftSettings settings) const
{
    return detail::price_with_greeks(*this, option, context, greeks, settings, [&](const auto& engine, const PricingContext& shifted, GreeksRequest request) { return price_binomial(option, shifted, engine.settings(), request); });
}

} // namespace kiyosi
