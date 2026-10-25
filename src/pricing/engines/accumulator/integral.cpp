#include <kiyosi/pricing/engines/accumulator/integral.hpp>

#include <algorithm>
#include <array>
#include <cmath>
#include <map>
#include <numeric>
#include <optional>
#include <tuple>
#include <vector>

#include "../../detail/numerical_greeks.hpp"

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

    double integrate(double log_spot, double strike, double drift, double deviation, double discount,
                     std::span<const IntervalMoments> weights, const std::array<double, 2>& tails, std::size_t first_cell = 0) const
    {
        Sum sum;
        sum.add(integrate_terminal(exact, log_spot, strike, drift, deviation, discount));
        if (!continuation.cells.empty()) sum.add(integrate_curve(continuation, weights, tails, discount, first_cell));
        return sum.total();
    }
};

using Scenario = std::tuple<Timestamp, BlackScholesMertonParameters>;

struct PreparedCurve {
    Scenario scenario;
    std::vector<double> nodes;
    std::array<Layer, 2> layers;
    double strike;
    double drift;
    double deviation;
    double discount;

    double evaluate(double spot, double quantity) const
    {
        const double current = log_price_ratio(spot, strike);
        const auto [first, last] = active_cells(nodes, current + drift, deviation);
        std::vector<IntervalMoments> weights;
        weights.reserve(last - first);
        for (std::size_t cell = first; cell < last; ++cell)
            weights.push_back(cell_moments(nodes[cell], nodes[cell + 1], current + drift, deviation));
        const std::array tails{log_normal_cdf((nodes.front() - current - drift) / deviation),
                               log_normal_cdf((current + drift - nodes.back()) / deviation)};
        return quantity * layers[0].integrate(current, strike, drift, deviation, discount, weights, tails, first) +
               layers[1].integrate(current, strike, drift, deviation, discount, weights, tails, first);
    }
};

void apply_observation(const Accumulator& option, const std::vector<double>& assets, double log_lower,
                       std::span<const Curve> continuation, double remaining_time,
                       std::size_t remaining_observations, double rate, double dividend,
                       std::array<Layer, 2>& result)
{
    const double strike = option.strike();
    const double knock_out = log_price_ratio(option.knock_out_level(), strike);
    const double low_cash = scaled_exponential(-strike, -rate * remaining_time);
    const double low_asset = std::exp(-dividend * remaining_time);
    // The low-price limit keeps the stock term as well as the cash from all remaining purchases.
    const double low_quantity = static_cast<double>(remaining_observations) *
                                accumulator_purchase_quantity(0.0, strike, option.daily_quantity(), option.acceleration_factor());
    for (std::size_t layer = 0; layer < continuation.size(); ++layer) {
        result[layer].continuation.cells.resize(assets.size());
        const double quantity = layer == 0 ? 1.0 : low_quantity;
        result[layer].continuation.low = quantity * low_cash;
        result[layer].continuation.high = 0.0;
        result[layer].exact.clear();
        result[layer].exact.push_back({-infinity, log_lower, 0.0, quantity * low_asset});
    }
    result[0].exact.push_back({knock_out, infinity, -strike, 1.0});
    for (std::size_t cell = 0; cell < assets.size(); ++cell) {
        const double asset = assets[cell];
        Polynomial slope{}, intercept{};
        if (!accumulator_knocked_out(asset, option.knock_out_level())) {
            slope = continuation[0].cells[cell];
            if (continuation.size() == 2) {
                intercept = continuation[1].cells[cell];
                const double purchase = accumulator_purchase_quantity(asset, strike, option.daily_quantity(), option.acceleration_factor());
                for (std::size_t power = 0; power < intercept.size(); ++power)
                    intercept[power] += purchase * slope[power];
            }
        }
        result[0].continuation.cells[cell] = slope;
        if (continuation.size() == 2) result[1].continuation.cells[cell] = intercept;
    }
}

Result<double> integrate_accumulator(const Accumulator& option, const PricingContext& context,
                                     const QuadratureSettings& settings, std::optional<PreparedCurve>& prepared)
{
    if (const auto valid = validate_valuation_within_instrument_life(context.valuation_time(), option.effective_date(), option.expiry_date()); !valid)
        return std::unexpected(valid.error());
    if (const auto valid = validate_trading_expiry(context.calendar(), option.expiry_date()); !valid)
        return std::unexpected(valid.error());
    const auto initial = accumulator_initial_state(option, context);
    if (initial.settlement) return checked_price(*initial.settlement);
    const Scenario scenario{context.valuation_time(), context.model_parameters()};
    // Quantity is resolved for each spot; both future affine layers are independent of it.
    if (prepared && prepared->scenario == scenario)
        return checked_price(prepared->evaluate(context.spot_price(), initial.quantity));

    const auto dates = trading_dates(context.calendar(), context.valuation_time(), option.expiry_date());
    const double strike = option.strike();
    const double knock_out = log_price_ratio(option.knock_out_level(), strike);
    const double rate = (*context.model_parameters().risk_free_rate());
    const double dividend = (*context.model_parameters().dividend_yield());
    const double sigma = context.model_parameters().volatility();
    const double drift_rate = rate - dividend - 0.5 * sigma * sigma;
    const double current = log_price_ratio(context.spot_price(), strike);
    const double horizon = actual_365_fixed_year_fraction(context.valuation_time(), option.expiry_date());
    const double accelerated = accumulator_purchase_quantity(0.0, strike, option.daily_quantity(), option.acceleration_factor());
    std::array<Layer, 2> layers;
    const std::size_t layer_count = accumulator_accrues(option) ? 2 : 1;
    layers[0].exact.push_back({-infinity, infinity, -strike, 1.0});
    if (layer_count == 2) {
        layers[1].exact.push_back({-infinity, std::min(0.0, knock_out), -strike * accelerated, accelerated});
        if (knock_out > 0.0)
            layers[1].exact.push_back({0.0, knock_out, -strike * option.daily_quantity(), option.daily_quantity()});
    }

    if (dates.size() == 1) {
        const double deviation = sigma * std::sqrt(horizon);
        const double drift = drift_rate * horizon;
        if (!(deviation > 0.0) || !std::isfinite(deviation) || !std::isfinite(drift))
            return std::unexpected(Error{ErrorCategory::invalid_result, "quadrature transition is not representable"});
        return checked_price(initial.quantity * layers[0].integrate(current, strike, drift, deviation, -rate * horizon, {}, {}) +
                             layers[1].integrate(current, strike, drift, deviation, -rate * horizon, {}, {}));
    }

    const double extent = settings.integration_standard_deviations * sigma * std::sqrt(horizon);
    const double lower = std::min(0.0, knock_out) + std::min(0.0, drift_rate * horizon) - extent;
    const double upper = std::max(0.0, knock_out) + std::max(0.0, drift_rate * horizon) + extent;
    auto grid = integration_nodes(lower, upper, settings.asset_step_count, std::array{0.0, knock_out});
    if (!grid) return std::unexpected(grid.error());
    auto nodes = std::move(*grid);
    const auto assets = cell_assets(nodes, strike);
    const CubicInterpolation interpolation{nodes};

    std::map<double, Transition> transitions;
    std::array<Curve, 2> continuation;
    std::array<std::vector<double>, 2> values{std::vector<double>(nodes.size()), std::vector<double>(layer_count == 2 ? nodes.size() : 0)};
    for (std::size_t event = dates.size(); event-- > 0;) {
        const bool final = event + 1 == dates.size();
        if (!final)
            apply_observation(option, assets, nodes.front(), std::span{continuation}.first(layer_count),
                              actual_365_fixed_year_fraction(dates[event], option.expiry_date()), dates.size() - event, rate, dividend, layers);
        const double time = event == 0 ? actual_365_fixed_year_fraction(context.valuation_time(), dates[event])
                                       : actual_365_fixed_year_fraction(dates[event - 1], dates[event]);
        const double drift = drift_rate * time;
        const double deviation = sigma * std::sqrt(time);
        const double discount = -rate * time;
        if (!(deviation > 0.0) || !std::isfinite(deviation) || !std::isfinite(drift))
            return std::unexpected(Error{ErrorCategory::invalid_result, "quadrature transition is not representable"});
        if (event == 0) {
            prepared = PreparedCurve{scenario, std::move(nodes), std::move(layers), strike, drift, deviation, discount};
            return checked_price(prepared->evaluate(context.spot_price(), initial.quantity));
        }
        const Transition* transition = nullptr;
        if (!final) {
            auto found = transitions.find(time);
            if (found == transitions.end()) found = transitions.emplace(time, transition_weights(nodes, drift, deviation)).first;
            transition = &found->second;
            if (layer_count == 2) integrate_curve_pair(layers[0].continuation, layers[1].continuation, *transition, discount, values[0], values[1]);
            else integrate_curves(layers[0].continuation, *transition, discount, values[0]);
        }
        for_each_target(nodes.size(), [&](std::size_t target) {
            for (std::size_t layer = 0; layer < layer_count; ++layer) {
                Sum sum;
                sum.add(integrate_terminal(layers[layer].exact, nodes[target], strike, drift, deviation, discount));
                if (!final) sum.add(values[layer][target]);
                values[layer][target] = sum.total();
            }
        });
        for (std::size_t layer = 0; layer < layer_count; ++layer)
            if (!std::ranges::all_of(values[layer], [](double value) { return std::isfinite(value); }))
                return std::unexpected(Error{ErrorCategory::invalid_result, "quadrature continuation is non-finite"});
        for (std::size_t layer = 0; layer < layer_count; ++layer)
            interpolation.interpolate(values[layer], 0.0, 0.0, continuation[layer]);
    }
    return std::unexpected(Error{ErrorCategory::invalid_result, "quadrature has no future observation"});
}

} // namespace

Result<double> QuadratureAccumulatorEngine::price(const Accumulator& option, const PricingContext& context) const
{
    if (auto valid = detail::require_flat_rates(context); !valid) return std::unexpected(valid.error());
    std::optional<PreparedCurve> prepared;
    return integrate_accumulator(option, context, settings_, prepared);
}

Result<PricingResult> QuadratureAccumulatorEngine::price_with_greeks(
    const Accumulator& option, const PricingContext& context, GreeksRequest greeks, NumericalShiftSettings settings) const
{
    if (auto valid = detail::require_flat_rates(context); !valid) return std::unexpected(valid.error());
    return detail::evaluate_greeks(*this, option, context, greeks, settings,
                                   [&](const auto& engine) -> Result<PricingResult> {
                                       std::optional<PreparedCurve> prepared;
                                       const auto price = [&](const PricingContext& shifted) {
                                           return integrate_accumulator(option, shifted, engine.settings(), prepared);
                                       };
                                       const auto value = price(context);
                                       if (!value) return std::unexpected(value.error());
                                       return detail::complete_greeks(price, option, context, greeks, settings, *make_pricing_result(*value));
                                   });
}

} // namespace kiyosi
