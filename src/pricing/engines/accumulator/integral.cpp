#include <kiyosi/pricing/engines/accumulator/integral.hpp>

#include <algorithm>
#include <array>
#include <cmath>
#include <map>
#include <numeric>
#include <vector>

#include <kiyosi/pricing/detail/numerical_greeks.hpp>

#include "../../detail/accumulator_state.hpp"
#include "../../detail/calendar_dates.hpp"
#include "../../detail/quadrature.hpp"

namespace kiyosi {
namespace {
using namespace detail;
using namespace detail::quadrature;

struct Layer {
    Curve continuation;
    std::vector<TerminalPiece> exact;
};

std::array<Layer, 2> apply_observation(const Accumulator& option, const std::vector<double>& nodes,
                                       const std::array<Curve, 2>& continuation, double remaining_time,
                                       std::size_t remaining_observations, double rate, double dividend)
{
    const double strike = option.strike();
    const double knock_out = log_price_ratio(option.knock_out_level(), strike);
    const double low_cash = signed_exponential(-strike, -rate * remaining_time);
    const double low_asset = std::exp(-dividend * remaining_time);
    // The low-price limit keeps the stock term as well as the cash from all remaining purchases.
    const double low_quantity = static_cast<double>(remaining_observations) *
                                accumulator_purchase_quantity(0.0, strike, option.daily_quantity(), option.acceleration_factor());
    std::array<Layer, 2> result;
    for (std::size_t layer = 0; layer < result.size(); ++layer) {
        result[layer].continuation.cells.reserve(nodes.size() - 1);
        const double quantity = layer == 0 ? 1.0 : low_quantity;
        result[layer].continuation.low = quantity * low_cash;
        result[layer].exact.push_back({-infinity, nodes.front(), 0.0, quantity * low_asset});
    }
    result[0].exact.push_back({knock_out, infinity, -strike, 1.0});
    for (std::size_t cell = 0; cell + 1 < nodes.size(); ++cell) {
        const double asset = strike * std::exp(std::midpoint(nodes[cell], nodes[cell + 1]));
        Polynomial slope{}, intercept{};
        if (!accumulator_knocked_out(asset, option.knock_out_level())) {
            slope = continuation[0].cells[cell];
            intercept = continuation[1].cells[cell];
            const double purchase = accumulator_purchase_quantity(asset, strike, option.daily_quantity(), option.acceleration_factor());
            for (std::size_t power = 0; power < intercept.size(); ++power)
                intercept[power] += purchase * slope[power];
        }
        result[0].continuation.cells.push_back(slope);
        result[1].continuation.cells.push_back(intercept);
    }
    return result;
}

Result<double> integrate_accumulator(const Accumulator& option, const PricingContext& context,
                                     const QuadratureSettings& settings)
{
    if (const auto valid = validate_valuation_within_instrument_life(context.valuation_time(), option.effective_date(), option.expiry_date()); !valid)
        return std::unexpected(valid.error());
    if (const auto valid = validate_trading_expiry(context.calendar(), option.expiry_date()); !valid)
        return std::unexpected(valid.error());
    const auto initial = accumulator_initial_state(option, context);
    if (initial.settlement) return checked_price(*initial.settlement);

    const auto dates = trading_dates(context.calendar(), context.valuation_time(), option.expiry_date());
    const double strike = option.strike();
    const double knock_out = log_price_ratio(option.knock_out_level(), strike);
    const double rate = context.model_parameters().risk_free_rate();
    const double dividend = context.model_parameters().dividend_yield();
    const double sigma = context.model_parameters().volatility();
    const double drift_rate = rate - dividend - 0.5 * sigma * sigma;
    const double current = log_price_ratio(context.spot_price(), strike);
    const double horizon = actual_365_fixed_year_fraction(context.valuation_time(), option.expiry_date());
    const double accelerated = accumulator_purchase_quantity(0.0, strike, option.daily_quantity(), option.acceleration_factor());
    std::array<Layer, 2> layers;
    layers[0].exact.push_back({-infinity, infinity, -strike, 1.0});
    layers[1].exact.push_back({-infinity, std::min(0.0, knock_out), -strike * accelerated, accelerated});
    if (knock_out > 0.0)
        layers[1].exact.push_back({0.0, knock_out, -strike * option.daily_quantity(), option.daily_quantity()});

    const auto integrate = [&](const Layer& layer, double log_spot, double drift, double deviation, double discount,
                               std::span<const IntervalMoments> weights, const std::array<double, 2>& tails) {
        Sum sum;
        sum.add(integrate_terminal(layer.exact, log_spot, strike, drift, deviation, discount));
        if (!layer.continuation.cells.empty()) sum.add(integrate_curve(layer.continuation, weights, tails, discount));
        return sum.total();
    };
    if (dates.size() == 1) {
        const double deviation = sigma * std::sqrt(horizon);
        const double drift = drift_rate * horizon;
        if (!(deviation > 0.0) || !std::isfinite(deviation) || !std::isfinite(drift))
            return std::unexpected(Error{ErrorCategory::invalid_result, "quadrature transition is not representable"});
        return checked_price(initial.quantity * integrate(layers[0], current, drift, deviation, -rate * horizon, {}, {}) +
                             integrate(layers[1], current, drift, deviation, -rate * horizon, {}, {}));
    }

    const double extent = settings.integration_standard_deviations * sigma * std::sqrt(horizon);
    const double lower = std::min(0.0, knock_out) + std::min(0.0, drift_rate * horizon) - extent;
    const double upper = std::max(0.0, knock_out) + std::max(0.0, drift_rate * horizon) + extent;
    if (!std::isfinite(lower) || !std::isfinite(upper) || lower >= upper)
        return std::unexpected(Error{ErrorCategory::invalid_result, "quadrature integration domain is not representable"});
    std::vector<double> nodes;
    const auto steps = static_cast<std::size_t>(settings.asset_step_count);
    nodes.reserve(steps + 3);
    for (std::size_t index = 0; index <= steps; ++index)
        nodes.push_back(std::lerp(lower, upper, static_cast<double>(index) / static_cast<double>(steps)));
    nodes.insert(nodes.end(), {0.0, knock_out});
    std::ranges::sort(nodes);
    nodes.erase(std::ranges::unique(nodes).begin(), nodes.end());
    if (nodes.size() < 4)
        return std::unexpected(Error{ErrorCategory::invalid_result, "quadrature grid has insufficient representable nodes"});

    std::map<double, Transition> transitions;
    std::array<Curve, 2> continuation;
    std::array<std::vector<double>, 2> values{std::vector<double>(nodes.size()), std::vector<double>(nodes.size())};
    for (std::size_t event = dates.size(); event-- > 0;) {
        const bool final = event + 1 == dates.size();
        if (!final)
            layers = apply_observation(option, nodes, continuation,
                                       actual_365_fixed_year_fraction(dates[event], option.expiry_date()), dates.size() - event, rate, dividend);
        const double time = event == 0 ? actual_365_fixed_year_fraction(context.valuation_time(), dates[event])
                                       : actual_365_fixed_year_fraction(dates[event - 1], dates[event]);
        const double drift = drift_rate * time;
        const double deviation = sigma * std::sqrt(time);
        const double discount = -rate * time;
        if (!(deviation > 0.0) || !std::isfinite(deviation) || !std::isfinite(drift))
            return std::unexpected(Error{ErrorCategory::invalid_result, "quadrature transition is not representable"});
        if (event == 0) {
            std::vector<IntervalMoments> weights;
            weights.reserve(nodes.size() - 1);
            for (std::size_t cell = 0; cell + 1 < nodes.size(); ++cell)
                weights.push_back(cell_moments(nodes[cell], nodes[cell + 1], current + drift, deviation));
            const std::array tails{log_normal_cdf((nodes.front() - current - drift) / deviation),
                                   log_normal_cdf((current + drift - nodes.back()) / deviation)};
            return checked_price(initial.quantity * integrate(layers[0], current, drift, deviation, discount, weights, tails) +
                                 integrate(layers[1], current, drift, deviation, discount, weights, tails));
        }
        const Transition* transition = nullptr;
        if (!final) {
            auto found = transitions.find(time);
            if (found == transitions.end()) found = transitions.emplace(time, transition_weights(nodes, drift, deviation)).first;
            transition = &found->second;
            for (std::size_t layer = 0; layer < layers.size(); ++layer)
                integrate_curves(layers[layer].continuation, *transition, discount, values[layer]);
        }
        for (std::size_t target = 0; target < nodes.size(); ++target) {
            for (std::size_t layer = 0; layer < layers.size(); ++layer) {
                Sum sum;
                sum.add(integrate_terminal(layers[layer].exact, nodes[target], strike, drift, deviation, discount));
                if (!final) sum.add(values[layer][target]);
                values[layer][target] = sum.total();
                if (!std::isfinite(values[layer][target]))
                    return std::unexpected(Error{ErrorCategory::invalid_result, "quadrature continuation is non-finite"});
            }
        }
        for (std::size_t layer = 0; layer < layers.size(); ++layer)
            continuation[layer] = interpolate(nodes, values[layer], 0.0, 0.0);
    }
    return std::unexpected(Error{ErrorCategory::invalid_result, "quadrature has no future observation"});
}

} // namespace

Result<double> QuadratureAccumulatorEngine::price(const Accumulator& option, const PricingContext& context) const
{
    return integrate_accumulator(option, context, settings_);
}

Result<PricingResult> QuadratureAccumulatorEngine::price_with_greeks(
    const Accumulator& option, const PricingContext& context, GreeksRequest greeks, NumericalShiftSettings settings) const
{
    return detail::price_with_greeks(*this, option, context, greeks, settings);
}

} // namespace kiyosi
