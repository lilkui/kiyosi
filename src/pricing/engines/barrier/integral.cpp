#include <kiyosi/pricing/engines/barrier/integral.hpp>

#include <algorithm>
#include <array>
#include <cmath>
#include <map>
#include <numeric>
#include <optional>
#include <tuple>
#include <vector>

#include "../../detail/barrier_settlement.hpp"
#include "../../detail/numerical_greeks.hpp"
#include "../../detail/quadrature.hpp"

namespace kiyosi {
namespace {
using namespace detail;
using namespace detail::quadrature;

struct Layer {
    Curve curve;
    std::vector<TerminalPiece> exact;

    double integrate(double current, double anchor, double drift, double deviation, double discount,
                     std::span<const IntervalMoments> weights = {}, std::size_t first = 0) const
    {
        Sum sum;
        sum.add(integrate_terminal(exact, current, anchor, drift, deviation, discount));
        if (!curve.cells.empty()) sum.add(integrate_curve(curve, weights, {}, discount, first));
        return sum.total();
    }
};

using Scenario = std::tuple<Timestamp, BlackScholesMertonParameters>;

struct PreparedCurve {
    Scenario scenario;
    std::vector<double> nodes;
    Layer layer;
    double anchor;
    double drift;
    double deviation;
    double discount;

    double evaluate(double spot) const
    {
        const double current = log_price_ratio(spot, anchor);
        if (layer.curve.cells.empty())
            return layer.integrate(current, anchor, drift, deviation, discount);
        const auto [first, last] = active_cells(nodes, current + drift, deviation);
        std::vector<IntervalMoments> weights;
        weights.reserve(last - first);
        for (std::size_t cell = first; cell < last; ++cell)
            weights.push_back(cell_moments(nodes[cell], nodes[cell + 1], current + drift, deviation));
        return layer.integrate(current, anchor, drift, deviation, discount, weights, first);
    }
};

Result<double> continuous_price(const BarrierOption& option, const PricingContext& context,
                                const QuadratureSettings& settings)
{
    const auto& parameters = context.model_parameters();
    const double time = actual_365_fixed_year_fraction(context.valuation_time(), option.expiry_date());
    const double sigma = parameters.volatility();
    const double width = sigma * std::sqrt(time);
    const double drift = ((*parameters.risk_free_rate()) - (*parameters.dividend_yield()) - 0.5 * sigma * sigma) * time;
    const double barrier = log_price_ratio(option.barrier_level(), context.spot_price());
    const double strike = log_price_ratio(option.strike(), context.spot_price());
    if (!(width > 0.0) || !std::isfinite(width) || !std::isfinite(drift))
        return std::unexpected(Error{ErrorCategory::invalid_result, "quadrature transition is not representable"});
    const double extent = settings.integration_standard_deviations;
    const double upper = extent + (option.option_type() == OptionType::call ? width : 0.0);
    std::vector<double> breaks{-extent, upper};
    for (const double point : {(strike - drift) / width, (barrier - drift) / width})
        if (point > -extent && point < upper) breaks.push_back(point);
    std::ranges::sort(breaks);
    const auto duplicate_breaks = std::ranges::unique(breaks);
    breaks.erase(duplicate_breaks.begin(), duplicate_breaks.end());
    const bool knock_in = option.barrier_terms().is_knock_in();
    const bool up = option.barrier_terms().is_up();
    const double sign = option.option_type() == OptionType::call ? 1.0 : -1.0;
    const double log_discount = -(*parameters.risk_free_rate()) * time;
    const auto integrand = [&](double z) {
        const double terminal = drift + width * z;
        const bool live = up ? terminal < barrier : terminal > barrier;
        const double exponent = live ? -2.0 * (barrier / width) * ((barrier - terminal) / width) : 0.0;
        const double log_survival = live ? std::log(-std::expm1(exponent)) : -infinity;
        const double log_density = log_discount - 0.5 * z * z + std::log(inverse_sqrt_two_pi);
        const double distance = terminal - strike;
        double value = 0.0;
        if (sign * distance > 0.0) {
            const double log_payoff = std::log(option.strike()) +
                                      (distance > 0.0 ? distance + std::log(-std::expm1(-distance))
                                                      : std::log(-std::expm1(distance)));
            value = std::exp(log_payoff + (knock_in ? exponent : log_survival) + log_density);
        }
        if (knock_in || option.rebate_timing() == RebateTiming::at_expiry)
            value += scaled_exponential(option.rebate(), (knock_in ? log_survival : exponent) + log_density);
        return value;
    };
    Sum result;
    const auto panels = static_cast<std::size_t>(settings.asset_step_count) + settings.asset_step_count % 2;
    for (std::size_t section = 1; section < breaks.size(); ++section) {
        const double lower = breaks[section - 1], upper_bound = breaks[section];
        const double step = (upper_bound - lower) / static_cast<double>(panels);
        Sum sum;
        sum.add(integrand(lower));
        sum.add(integrand(upper_bound));
        for (std::size_t index = 1; index < panels; ++index)
            sum.add((index % 2 == 0 ? 2.0 : 4.0) * integrand(lower + static_cast<double>(index) * step));
        result.add(sum.total() * step / 3.0);
    }
    if (!knock_in && option.rebate_timing() == RebateTiming::at_hit && option.rebate() != 0.0) {
        const auto rebate = barrier_hit_value(option.rebate(), std::abs(barrier), up, drift / time,
                                              sigma, time, (*parameters.risk_free_rate()));
        if (!rebate) return std::unexpected(rebate.error());
        result.add(*rebate);
    }
    return checked_price(result.total());
}

std::array<double, 2> vanilla_limit(const BarrierOption& option, double remaining, const PricingContext& context, bool high)
{
    const double sign = option.option_type() == OptionType::call ? 1.0 : -1.0;
    if ((sign > 0.0) != high) return {};
    return {-sign * scaled_exponential(option.strike(), -(*context.model_parameters().risk_free_rate()) * remaining),
            sign * std::exp(-(*context.model_parameters().dividend_yield()) * remaining)};
}

std::array<double, 2> event_limit(const BarrierOption& option, double remaining, const PricingContext& context, bool high)
{
    const bool hit = option.barrier_terms().is_up() == high;
    if (hit == option.barrier_terms().is_knock_in()) return vanilla_limit(option, remaining, context, high);
    const double discount = !option.barrier_terms().is_knock_in() && option.rebate_timing() == RebateTiming::at_hit
                                ? 0.0
                                : -(*context.model_parameters().risk_free_rate()) * remaining;
    return {scaled_exponential(option.rebate(), discount), 0.0};
}

Layer terminal_layer(const BarrierOption& option)
{
    const double barrier = log_price_ratio(option.barrier_level(), option.strike());
    std::vector<double> edges{-infinity, std::min(0.0, barrier), std::max(0.0, barrier), infinity};
    const auto duplicate_edges = std::ranges::unique(edges);
    edges.erase(duplicate_edges.begin(), duplicate_edges.end());
    Layer layer;
    for (std::size_t cell = 1; cell < edges.size(); ++cell) {
        const double point = edges[cell - 1] == -infinity ? edges[cell] - 1.0
                             : edges[cell] == infinity    ? edges[cell - 1] + 1.0
                                                          : std::midpoint(edges[cell - 1], edges[cell]);
        const bool hit = option.barrier_terms().is_up() ? point > barrier : point < barrier;
        double cash = 0.0, asset = 0.0;
        if (hit == option.barrier_terms().is_knock_in()) {
            const double sign = option.option_type() == OptionType::call ? 1.0 : -1.0;
            if (sign * point > 0.0) {
                cash = -sign * option.strike();
                asset = sign;
            }
        } else cash = option.rebate();
        layer.exact.push_back({edges[cell - 1], edges[cell], cash, asset});
    }
    return layer;
}

Result<Layer> event_layer(const BarrierOption& option, const PricingContext& context, Date date,
                          const std::vector<double>& nodes, const CubicInterpolation& interpolation,
                          std::span<const double> continuation)
{
    const double remaining = actual_365_fixed_year_fraction(date, option.expiry_date());
    const bool knock_in = option.barrier_terms().is_knock_in();
    const double rate = (*context.model_parameters().risk_free_rate());
    const double rebate = scaled_exponential(option.rebate(),
                                             !knock_in && option.rebate_timing() == RebateTiming::at_hit ? 0.0 : -rate * remaining);
    Curve vanilla_curve;
    if (knock_in || continuation.empty()) {
        std::vector<double> vanilla(nodes.size());
        const auto european = make_european_option(option.option_type(), option.strike(), option.effective_date(), option.expiry_date());
        for (std::size_t index = 0; index < nodes.size(); ++index) {
            const double spot = scaled_exponential(option.strike(), nodes[index]);
            if (!std::isfinite(spot) || spot <= 0.0)
                return std::unexpected(Error{ErrorCategory::invalid_result, "quadrature asset grid is not representable"});
            const auto shifted = context.replace({.spot_price = spot, .valuation_time = start_of_day(date)});
            if (!shifted) return std::unexpected(shifted.error());
            const auto value = price_at_volatility(*european, *shifted, context.model_parameters().volatility(), GreeksRequest{});
            if (!value) return std::unexpected(value.error());
            vanilla[index] = value->price();
        }
        interpolation.interpolate(vanilla, 0.0, 0.0, vanilla_curve);
    }
    Layer layer;
    if (continuation.empty()) {
        if (knock_in) layer.curve.cells.assign(nodes.size() - 1, Polynomial{rebate});
        else layer.curve = vanilla_curve;
    } else interpolation.interpolate(continuation, 0.0, 0.0, layer.curve);
    const double barrier = log_price_ratio(option.barrier_level(), option.strike());
    for (std::size_t cell = 0; cell < layer.curve.cells.size(); ++cell) {
        const double point = std::midpoint(nodes[cell], nodes[cell + 1]);
        if (option.barrier_terms().is_up() ? point > barrier : point < barrier)
            layer.curve.cells[cell] = knock_in ? vanilla_curve.cells[cell] : Polynomial{rebate};
    }
    const auto low = event_limit(option, remaining, context, false);
    const auto high = event_limit(option, remaining, context, true);
    layer.exact = {{-infinity, nodes.front(), low[0], low[1]}, {nodes.back(), infinity, high[0], high[1]}};
    return layer;
}

Result<double> scheduled_price(const BarrierOption& option, const PricingContext& context,
                               const QuadratureSettings& settings, std::optional<PreparedCurve>& prepared)
{
    const Scenario scenario{context.valuation_time(), context.model_parameters()};
    if (prepared && prepared->scenario == scenario) return checked_price(prepared->evaluate(context.spot_price()));
    std::vector<Date> dates;
    for (const auto date : option.observation_dates())
        if (start_of_day(date) > context.valuation_time()) dates.push_back(date);
    const auto& parameters = context.model_parameters();
    const double sigma = parameters.volatility();
    const double drift_rate = (*parameters.risk_free_rate()) - (*parameters.dividend_yield()) - 0.5 * sigma * sigma;
    const double horizon = actual_365_fixed_year_fraction(context.valuation_time(), option.expiry_date());
    const double extent = settings.integration_standard_deviations * sigma * std::sqrt(horizon);
    const double barrier = log_price_ratio(option.barrier_level(), option.strike());
    const double current = log_price_ratio(context.spot_price(), option.strike());
    if (dates.size() == 1 && dates.front() == option.expiry_date()) {
        const double deviation = sigma * std::sqrt(horizon);
        const double drift = drift_rate * horizon;
        if (!(deviation > 0.0) || !std::isfinite(deviation) || !std::isfinite(drift))
            return std::unexpected(Error{ErrorCategory::invalid_result, "quadrature transition is not representable"});
        return checked_price(terminal_layer(option).integrate(current, option.strike(), drift, deviation,
                                                              -(*parameters.risk_free_rate()) * horizon));
    }
    const double lower = std::min({0.0, barrier, current}) + std::min(0.0, drift_rate * horizon) - extent;
    const double upper = std::max({0.0, barrier, current}) + std::max(0.0, drift_rate * horizon) + extent;
    auto grid = integration_nodes(lower, upper, settings.asset_step_count, std::array{0.0, barrier});
    if (!grid) return std::unexpected(grid.error());
    auto nodes = std::move(*grid);
    const CubicInterpolation interpolation{nodes};
    Layer layer;
    std::vector<double> values(nodes.size());
    std::map<double, Transition> transitions;
    for (std::size_t event = dates.size(); event-- > 0;) {
        const bool final = event + 1 == dates.size();
        if (final && dates[event] == option.expiry_date()) layer = terminal_layer(option);
        else {
            auto next = event_layer(option, context, dates[event], nodes, interpolation,
                                    final ? std::span<const double>{} : std::span<const double>{values});
            if (!next) return std::unexpected(next.error());
            layer = std::move(*next);
        }
        const double time = event == 0 ? actual_365_fixed_year_fraction(context.valuation_time(), dates[event])
                                       : actual_365_fixed_year_fraction(dates[event - 1], dates[event]);
        const double drift = drift_rate * time;
        const double deviation = sigma * std::sqrt(time);
        const double discount = -(*parameters.risk_free_rate()) * time;
        if (!(deviation > 0.0) || !std::isfinite(deviation) || !std::isfinite(drift))
            return std::unexpected(Error{ErrorCategory::invalid_result, "quadrature transition is not representable"});
        if (event == 0) {
            prepared = PreparedCurve{scenario, std::move(nodes), std::move(layer), option.strike(), drift, deviation, discount};
            return checked_price(prepared->evaluate(context.spot_price()));
        }
        if (!layer.curve.cells.empty()) {
            auto found = transitions.find(time);
            if (found == transitions.end()) found = transitions.emplace(time, transition_weights(nodes, drift, deviation)).first;
            integrate_curves(layer.curve, found->second, discount, values);
        } else std::ranges::fill(values, 0.0);
        for_each_target(nodes.size(), [&](std::size_t target) {
            values[target] += integrate_terminal(layer.exact, nodes[target], option.strike(), drift, deviation, discount);
        });
        if (!std::ranges::all_of(values, [](double value) { return std::isfinite(value); }))
            return std::unexpected(Error{ErrorCategory::invalid_result, "quadrature continuation is non-finite"});
    }
    return std::unexpected(Error{ErrorCategory::invalid_result, "quadrature has no future observation"});
}

Result<double> price_barrier(const BarrierOption& option, const PricingContext& context,
                             const QuadratureSettings& settings, std::optional<PreparedCurve>& prepared, bool& schedule_validated)
{
    const auto valid = validate_valuation_within_instrument_life(context.valuation_time(), option.effective_date(), option.expiry_date());
    if (!valid) return std::unexpected(valid.error());
    if (!schedule_validated && option.observation_mode() == ObservationMode::scheduled) {
        const auto schedule = validate_observation_dates(option.observation_dates(), option.effective_date(), option.expiry_date(), context.calendar());
        if (!schedule) return std::unexpected(schedule.error());
        schedule_validated = true;
    }
    if (const auto settled = resolved_barrier_price(option, context)) return *settled;
    if (context.valuation_time() == start_of_day(option.expiry_date()))
        return option.barrier_terms().is_knock_in() ? option.rebate()
                                                    : std::max((option.option_type() == OptionType::call ? 1.0 : -1.0) * (context.spot_price() - option.strike()), 0.0);
    return option.observation_mode() == ObservationMode::continuous
               ? continuous_price(option, context, settings)
               : scheduled_price(option, context, settings, prepared);
}

} // namespace

Result<double> QuadratureBarrierEngine::price(const BarrierOption& option, const PricingContext& context) const
{
    if (auto valid = detail::require_flat_rates(context); !valid) return std::unexpected(valid.error());
    std::optional<PreparedCurve> prepared;
    bool schedule_validated = false;
    return price_barrier(option, context, settings_, prepared, schedule_validated);
}

Result<PricingResult> QuadratureBarrierEngine::price_with_greeks(
    const BarrierOption& option, const PricingContext& context, GreeksRequest greeks, NumericalShiftSettings settings) const
{
    if (auto valid = detail::require_flat_rates(context); !valid) return std::unexpected(valid.error());
    return detail::evaluate_greeks(*this, option, context, greeks, settings,
                                   [&](const auto& engine) -> Result<PricingResult> {
                                       std::optional<PreparedCurve> prepared;
                                       bool schedule_validated = false;
                                       const auto price = [&](const PricingContext& shifted) {
                                           return price_barrier(option, shifted, engine.settings(), prepared, schedule_validated);
                                       };
                                       const auto value = price(context);
                                       if (!value) return std::unexpected(value.error());
                                       return detail::complete_greeks(price, option, context, greeks, settings, *make_pricing_result(*value));
                                   });
}

} // namespace kiyosi
