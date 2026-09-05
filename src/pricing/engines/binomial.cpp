#include <ito/pricing/engines/binomial.hpp>
#include "../detail/common.hpp"
#include <algorithm>
#include <array>
#include <cmath>
#include <limits>
#include <ranges>
#include <vector>

namespace ito {
using namespace detail;

template <typename Option>
result<PricingResult> price_binomial_american(
    const Option& option, const PricingContext& context, BinomialAmericanSettings settings)
{
    // ponytail: O(N²) rollback with O(N) memory; optimize to a recombining index kernel if profiling requires it.
    const auto valid_expiry = validate_expiry(context.valuation_date(), option.expiry());
    if (!valid_expiry) return std::unexpected(valid_expiry.error());
    if (settings.steps <= 0 || settings.steps > 1'000'000) {
        return std::unexpected(Error{error_category::invalid_parameter,
                                     "binomial step count must be between 1 and 1000000"});
    }

    const double spot = context.asset_price().value();
    const double strike = option.strike();
    const double sign = option.type() == option_type::call ? 1.0 : -1.0;
    const double time = actual_365(context.valuation_date(), option.expiry());
    if (time == 0.0) {
        auto output = PricingResult{std::max(sign * (spot - strike), 0.0),
                                    0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0};
        output.available = risk_bit(risk_measure::price);
        return output;
    }

    const double rate = context.parameters().risk_free_rate();
    const double dividend = context.parameters().dividend_yield();
    const double volatility = context.parameters().volatility();
    const double dt = time / static_cast<double>(settings.steps);
    const double root_dt = std::sqrt(dt);
    const double up = std::exp(volatility * root_dt);
    const double down = 1.0 / up;
    const double growth = std::exp((rate - dividend) * dt);
    const double discount = std::exp(-rate * dt);
    const double probability = (growth - down) / (up - down);
    if (!std::isfinite(up) || !std::isfinite(down) || !std::isfinite(discount) ||
        !std::isfinite(probability) || probability < 0.0 || probability > 1.0) {
        return std::unexpected(Error{error_category::invalid_result,
                                     "binomial parameters produced an unstable tree"});
    }

    std::vector<double> values(static_cast<std::size_t>(settings.steps) + 1);
    const double up_squared = up * up;
    double node_spot = spot * std::pow(down, settings.steps);
    if (!std::isfinite(up_squared) || !std::isfinite(node_spot)) {
        return std::unexpected(Error{error_category::invalid_result,
                                     "binomial tree produced a non-finite asset price"});
    }
    for (int node = 0; node <= settings.steps; ++node) {
        if (!std::isfinite(node_spot)) {
            return std::unexpected(Error{error_category::invalid_result,
                                         "binomial tree produced a non-finite asset price"});
        }
        values[static_cast<std::size_t>(node)] = std::max(sign * (node_spot - strike), 0.0);
        node_spot *= up_squared;
    }

    std::array<double, 3> level_two{};
    std::array<double, 2> level_one{};
    if (settings.steps == 1) level_one = {values[0], values[1]};
    if (settings.steps == 2) level_two = {values[0], values[1], values[2]};
    node_spot = spot * std::pow(down, settings.steps - 1);
    for (int level = settings.steps - 1; level >= 0; --level) {
        if (level < settings.steps - 1) node_spot *= up;
        double level_node_spot = node_spot;
        for (int node = 0; node <= level; ++node) {
            const std::size_t index = static_cast<std::size_t>(node);
            const double continuation = discount *
                                        (probability * values[index + 1] + (1.0 - probability) * values[index]);
            values[index] = std::max(continuation, sign * (level_node_spot - strike));
            if (!std::isfinite(values[index])) {
                return std::unexpected(Error{error_category::invalid_result,
                                             "binomial pricing produced a non-finite result"});
            }
            level_node_spot *= up_squared;
        }
        if (level == 2) {
            level_two = {values[0], values[1], values[2]};
        } else if (level == 1) {
            level_one = {values[0], values[1]};
        }
    }

    double delta = 0.0;
    double gamma = 0.0;
    if (settings.steps >= 1) {
        const double denominator = spot * (up - down);
        if (std::isfinite(denominator) && denominator != 0.0)
            delta = (level_one[1] - level_one[0]) / denominator;
    }
    if (settings.steps >= 2) {
        const double delta_up_denominator = spot * (up * up - 1.0);
        const double delta_down_denominator = spot * (1.0 - down * down);
        const double gamma_denominator = 0.5 * spot * (up * up - down * down);
        if (delta_up_denominator != 0.0 && delta_down_denominator != 0.0 && gamma_denominator != 0.0) {
            const double delta_up = (level_two[2] - level_two[1]) / delta_up_denominator;
            const double delta_down = (level_two[1] - level_two[0]) / delta_down_denominator;
            gamma = (delta_up - delta_down) / gamma_denominator;
        }
    }

    auto output = PricingResult{values[0], delta, gamma, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0};
    output.available = risk_bit(risk_measure::price) | risk_bit(risk_measure::delta) |
                       risk_bit(risk_measure::gamma);
    const std::array result_values{output.value, output.delta, output.gamma};
    if (!std::ranges::all_of(result_values, [](double value) { return std::isfinite(value); })) {
        return std::unexpected(Error{error_category::invalid_result,
                                     "binomial pricing produced a non-finite result"});
    }
    return output;
}

result<PricingResult> BinomialAmericanEngine::price(
    const EuropeanOption&, const PricingContext&) const
{
    return std::unexpected(Error{error_category::incompatible_exercise,
                                 "American engine requires an American exercise instrument"});
}

result<PricingResult> BinomialAmericanEngine::price(
    const EuropeanOption&, const PricingContext&, BinomialAmericanSettings) const
{
    return std::unexpected(Error{error_category::incompatible_exercise,
                                 "American engine requires an American exercise instrument"});
}

result<PricingResult> BinomialAmericanEngine::price(
    const AmericanOption& option, const PricingContext& context) const
{
    return select_outputs(price_binomial_american(option, context, settings_),
                          PricingRequest{supported_risk_measures}, supported_risk_measures);
}

result<PricingResult> BinomialAmericanEngine::price(
    const AmericanOption& option, const PricingContext& context, PricingRequest request) const
{
    return select_outputs(price_binomial_american(option, context, settings_),
                          request, supported_risk_measures);
}

result<PricingResult> BinomialAmericanEngine::price(
    const AmericanOption& option, const PricingContext& context, BinomialAmericanSettings settings) const
{
    return select_outputs(price_binomial_american(option, context, settings),
                          PricingRequest{supported_risk_measures}, supported_risk_measures);
}

result<PricingResult> BinomialAmericanEngine::price(
    const AmericanOption& option, const PricingContext& context,
    BinomialAmericanSettings settings, PricingRequest request) const
{
    return select_outputs(price_binomial_american(option, context, settings),
                          request, supported_risk_measures);
}

result<PricingResult> BinomialAmericanEngine::price(
    const EuropeanOption&, const PricingContext&, PricingRequest) const
{
    return std::unexpected(Error{error_category::incompatible_exercise,
                                 "American engine requires an American exercise instrument"});
}


} // namespace ito
