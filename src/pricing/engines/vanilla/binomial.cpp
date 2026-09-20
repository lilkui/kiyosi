#include <kiyosi/pricing/engines/vanilla/binomial.hpp>

#include <algorithm>
#include <array>
#include <cmath>
#include <optional>
#include <vector>

#include "../../detail/math.hpp"

namespace kiyosi {
using namespace detail;

template <typename Option>
Result<PricingResult> price_binomial(
    const Option& option, const PricingContext& context, BinomialSettings settings, bool american)
{
    // ponytail: O(N²) rollback with O(N) memory; optimize to a recombining index kernel if profiling requires it.
    const auto valid_expiry = validate_valuation_within_instrument_life(context.valuation_time(), option.effective_date(), option.expiry_date());
    if (!valid_expiry) return std::unexpected(valid_expiry.error());
    if (settings.step_count <= 0 || settings.step_count > 1'000'000) {
        return std::unexpected(Error{ErrorCategory::invalid_parameter,
                                     "binomial step count must be between 1 and 1000000"});
    }

    const double spot = context.spot_price();
    const double strike = option.strike();
    const double sign = option.option_type() == OptionType::call ? 1.0 : -1.0;
    const double time = actual_365_fixed_year_fraction(context.valuation_time(), option.expiry_date());
    if (time == 0.0) {
        return make_pricing_result(
            {{RiskMeasure::price, std::max(sign * (spot - strike), 0.0)}});
    }

    const double rate = context.model_parameters().risk_free_rate();
    const double dividend = context.model_parameters().dividend_yield();
    const double volatility = context.model_parameters().volatility();
    const double dt = time / static_cast<double>(settings.step_count);
    const double root_dt = std::sqrt(dt);
    const double up = std::exp(volatility * root_dt);
    const double down = 1.0 / up;
    const double growth = std::exp((rate - dividend) * dt);
    const double discount = std::exp(-rate * dt);
    const double probability = (growth - down) / (up - down);
    if (!std::isfinite(up) || !std::isfinite(down) || !std::isfinite(discount) ||
        !std::isfinite(probability) || probability < 0.0 || probability > 1.0) {
        return std::unexpected(Error{ErrorCategory::invalid_result,
                                     "binomial parameters produced an unstable tree"});
    }

    std::vector<double> values(static_cast<std::size_t>(settings.step_count) + 1);
    const double up_squared = up * up;
    double node_spot = spot * std::pow(down, settings.step_count);
    if (!std::isfinite(up_squared) || !std::isfinite(node_spot)) {
        return std::unexpected(Error{ErrorCategory::invalid_result,
                                     "binomial tree produced a non-finite asset price"});
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
    if (settings.step_count == 1) level_one = {values[0], values[1]};
    if (settings.step_count == 2) level_two = {values[0], values[1], values[2]};
    node_spot = spot * std::pow(down, settings.step_count - 1);
    for (int level = settings.step_count - 1; level >= 0; --level) {
        if (level < settings.step_count - 1) node_spot *= up;
        double level_node_spot = node_spot;
        for (int node = 0; node <= level; ++node) {
            const std::size_t index = static_cast<std::size_t>(node);
            const double continuation = discount *
                                        (probability * values[index + 1] + (1.0 - probability) * values[index]);
            values[index] = american ? std::max(continuation, sign * (level_node_spot - strike)) : continuation;
            if (!std::isfinite(values[index])) {
                return std::unexpected(Error{ErrorCategory::invalid_result,
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
    bool gamma_available = false;
    if (settings.step_count >= 1) {
        const double denominator = spot * (up - down);
        if (std::isfinite(denominator) && denominator != 0.0)
            delta = (level_one[1] - level_one[0]) / denominator;
    }
    if (settings.step_count >= 2) {
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

    auto output = make_pricing_result(
        {{RiskMeasure::price, values[0]},
         {RiskMeasure::delta, delta},
         {RiskMeasure::gamma,
          gamma_available ? std::optional<double>{gamma} : std::nullopt}});
    if (!output) return std::unexpected(output.error());
    if (!output->all_finite()) {
        return std::unexpected(Error{ErrorCategory::invalid_result,
                                     "binomial pricing produced a non-finite result"});
    }
    return output;
}

Result<PricingResult> CoxRossRubinsteinVanillaEngine::price_european(
    const EuropeanOption& option, const PricingContext& context) const
{
    return price_binomial(option, context, settings_, false);
}

Result<PricingResult> CoxRossRubinsteinVanillaEngine::price_american(
    const AmericanOption& option, const PricingContext& context) const
{
    return price_binomial(option, context, settings_, true);
}

} // namespace kiyosi
