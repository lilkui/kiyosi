#include <kiyosi/pricing/engines/autocallable/integral.hpp>

#include <algorithm>
#include <array>
#include <cmath>
#include <limits>
#include <map>
#include <numeric>
#include <vector>

#include <kiyosi/pricing/detail/autocallable_traits.hpp>
#include "../../detail/numerical_greeks.hpp"

#include "../../detail/autocallable_pricer.hpp"
#include "../../detail/calendar_dates.hpp"
#include "../../detail/math.hpp"
#include "../../detail/quadrature.hpp"

namespace kiyosi {
namespace {
using namespace detail;
using namespace detail::quadrature;
struct Event {
    Date date;
    double time;
    AutocallableEvent terms{};
};

double event_coupon(const AutocallableProgram& program, const AutocallableEvent& event, double asset)
{
    return event.active && program.carries_observation_coupon ? program_observation_coupon(event, asset) : 0.0;
}

std::array<double, 2> event_limits(const AutocallableProgram& program, const AutocallableEvent& event,
                                   const Curve& curve)
{
    const auto limit = [&](double asset, double continuation) {
        if (event.active && program_knocked_out(event, asset))
            return program.principal_ratio + program_observation_coupon(event, asset);
        return continuation + event_coupon(program, event, asset);
    };
    return {limit(0.0, curve.low), limit(infinity, curve.high)};
}

Curve apply_event(const AutocallableProgram& program, const AutocallableEvent& event,
                  const std::vector<double>& nodes, double anchor, const Curve& alive, const Curve& knocked_in,
                  bool transition)
{
    const auto limits = event_limits(program, event, alive);
    Curve result{{}, transition && program_knocked_in(program, 0.0, false, false) ? event_limits(program, event, knocked_in)[0] : limits[0], limits[1]};
    result.cells.reserve(alive.cells.size());
    for (std::size_t i = 0; i < alive.cells.size(); ++i) {
        const double asset = anchor * std::exp(std::midpoint(nodes[i], nodes[i + 1]));
        if (event.active && program_knocked_out(event, asset)) {
            result.cells.push_back({program.principal_ratio + program_observation_coupon(event, asset), 0.0, 0.0, 0.0});
        } else {
            const bool crossed = transition && program_knocked_in(program, asset, false, false);
            auto polynomial = crossed ? knocked_in.cells[i] : alive.cells[i];
            polynomial[0] += event_coupon(program, event, asset);
            result.cells.push_back(polynomial);
        }
    }
    return result;
}

std::vector<TerminalPiece> terminal_pieces(const AutocallableProgram& program, const AutocallableEvent& event,
                                           const std::vector<double>& breaks, double anchor, bool previously_in)
{
    std::vector<double> edges{-infinity};
    edges.insert(edges.end(), breaks.begin(), breaks.end());
    edges.push_back(infinity);
    std::vector<TerminalPiece> pieces;
    for (std::size_t i = 0; i + 1 < edges.size(); ++i) {
        const double log_asset = edges[i] == -infinity && edges[i + 1] == infinity ? 0.0 : edges[i] == -infinity  ? edges[i + 1] - 1.0
                                                                                       : edges[i + 1] == infinity ? edges[i] + 1.0
                                                                                                                  : std::midpoint(edges[i], edges[i + 1]);
        const double asset = anchor * std::exp(log_asset);
        double cash = 0.0;
        double weight = 0.0;
        if (event.active && program_knocked_out(event, asset)) {
            cash = program.principal_ratio + program_observation_coupon(event, asset);
        } else {
            const bool in = program_knocked_in(program, asset, previously_in, true);
            if (in && program.terminal_kind == AutocallableTerminalKind::downside_if_knocked_in &&
                asset > program.lower_strike && asset < program.upper_strike) {
                cash = program.principal_ratio - program.upper_strike / program.initial_spot;
                weight = 1.0 / program.initial_spot;
            } else {
                cash = program_terminal_settlement(program, asset, in);
            }
            cash += event_coupon(program, event, asset);
        }
        pieces.push_back({edges[i], edges[i + 1], cash, weight});
    }
    return pieces;
}

template <AutocallableProduct Note>
Result<double> integrate_autocallable(const Note& note, const PricingContext& context,
                                      const QuadratureSettings& settings)
{
    AutocallableContract<Note> contract{note, context.calendar()};
    const auto valid = contract.validate(context);
    if (!valid) return std::unexpected(valid.error());
    const auto program = autocallable_program(note);
    const auto initial = autocallable_initial_state(note, context, program);
    if (initial.settlement) return checked_price(*initial.settlement);
    if (const auto fixed = autocallable_fixed_value(note, context, program, initial)) return checked_price(*fixed);
    const bool layers = program.daily_knock_in && !initial.path.knocked_in && program_knock_in_affects_payoff(program);
    const auto dates = autocallable_event_dates(note, context, initial.next_observation, layers);
    const auto observations = note.observations_view();
    std::vector<Event> events;
    std::size_t observation = initial.next_observation;
    for (const auto date : dates) {
        AutocallableEvent event{};
        if (observation < observations.size() && observations[observation].date == date)
            event = autocallable_event(note, observation++);
        events.push_back({date, actual_365_fixed_year_fraction(context.valuation_time(), date), event});
    }
    const double anchor = program.initial_spot > 0.0 ? program.initial_spot : observations.empty() ? 1.0
                                                                                                   : observations.front().knock_out_level;
    const double rate = context.model_parameters().risk_free_rate();
    const double sigma = context.model_parameters().volatility();
    const double drift_rate = rate - context.model_parameters().dividend_yield() - 0.5 * sigma * sigma;
    std::vector<double> breaks;
    const auto add = [&](double level) { if (level > 0.0) breaks.push_back(log_price_ratio(level, anchor)); };
    if (program.has_knock_in) add(program.knock_in_level);
    if (program.terminal_kind == AutocallableTerminalKind::downside_if_knocked_in) {
        add(program.lower_strike);
        add(program.upper_strike);
    }
    for (const auto& event : events)
        if (event.terms.active) {
            add(event.terms.knock_out_level);
            if (event.terms.conditional_coupon) add(event.terms.coupon_barrier);
        }
    std::ranges::sort(breaks);
    breaks.erase(std::ranges::unique(breaks).begin(), breaks.end());
    const auto terminal = terminal_pieces(program, events.back().terms, breaks, anchor, initial.path.knocked_in);
    const auto terminal_in = layers ? terminal_pieces(program, events.back().terms, breaks, anchor, true) : std::vector<TerminalPiece>{};
    const double current = log_price_ratio(context.spot_price(), anchor);
    if (events.size() == 1) {
        const double time = events.front().time;
        return checked_price(initial.path.coupons + integrate_terminal(terminal, current, anchor, drift_rate * time,
                                                                       sigma * std::sqrt(time), -rate * time));
    }
    const double horizon = events.back().time;
    const double extent = settings.integration_standard_deviations * sigma * std::sqrt(horizon);
    const double lower = std::min(0.0, breaks.empty() ? 0.0 : breaks.front()) + std::min(0.0, drift_rate * horizon) - extent;
    const double upper = std::max(0.0, breaks.empty() ? 0.0 : breaks.back()) + std::max(0.0, drift_rate * horizon) + extent;
    const auto grid = integration_nodes(lower, upper, settings.asset_step_count, breaks);
    if (!grid) return std::unexpected(grid.error());
    const auto& nodes = *grid;
    std::map<double, Transition> transitions;
    Curve alive, knocked_in;
    std::vector<double> values(nodes.size()), values_in(layers ? nodes.size() : 0);
    for (std::size_t event_index = events.size(); event_index-- > 0;) {
        const auto& event = events[event_index];
        const bool final = event_index + 1 == events.size();
        if (!final) {
            Curve next_alive = apply_event(program, event.terms, nodes, anchor, alive, knocked_in, layers);
            if (layers) knocked_in = apply_event(program, event.terms, nodes, anchor, knocked_in, knocked_in, false);
            alive = std::move(next_alive);
        }
        const double time = event_index == 0 ? event.time : actual_365_fixed_year_fraction(events[event_index - 1].date, event.date);
        const double drift = drift_rate * time;
        const double deviation = sigma * std::sqrt(time);
        const double discount = -rate * time;
        if (!(deviation > 0.0) || !std::isfinite(deviation) || !std::isfinite(drift))
            return std::unexpected(Error{ErrorCategory::invalid_result, "quadrature transition is not representable"});
        if (event_index == 0) {
            std::vector<IntervalMoments> weights;
            weights.reserve(nodes.size() - 1);
            for (std::size_t cell = 0; cell + 1 < nodes.size(); ++cell)
                weights.push_back(cell_moments(nodes[cell], nodes[cell + 1], current + drift, deviation));
            return checked_price(initial.path.coupons + integrate_curve(alive, weights,
                                                                        {log_normal_cdf((nodes.front() - current - drift) / deviation),
                                                                         log_normal_cdf((current + drift - nodes.back()) / deviation)},
                                                                        discount));
        }
        const Transition* transition = nullptr;
        if (!final) {
            auto found = transitions.find(time);
            if (found == transitions.end()) found = transitions.emplace(time, transition_weights(nodes, drift, deviation)).first;
            transition = &found->second;
            integrate_curves(alive, *transition, discount, values);
            if (layers) integrate_curves(knocked_in, *transition, discount, values_in);
        }
        for (std::size_t target = 0; target < nodes.size(); ++target) {
            if (final) {
                values[target] = integrate_terminal(terminal, nodes[target], anchor, drift, deviation, discount);
                if (layers) values_in[target] = integrate_terminal(terminal_in, nodes[target], anchor, drift, deviation, discount);
            }
            if (!std::isfinite(values[target]) || (layers && !std::isfinite(values_in[target])))
                return std::unexpected(Error{ErrorCategory::invalid_result, "quadrature continuation is non-finite"});
        }
        const double low = final ? terminal.front().cash : alive.low;
        const double high = final ? terminal.back().cash : alive.high;
        alive = interpolate(nodes, values, signed_exponential(low, discount), signed_exponential(high, discount));
        if (layers) {
            const double low_in = final ? terminal_in.front().cash : knocked_in.low;
            const double high_in = final ? terminal_in.back().cash : knocked_in.high;
            knocked_in = interpolate(nodes, values_in, signed_exponential(low_in, discount), signed_exponential(high_in, discount));
        }
    }
    return std::unexpected(Error{ErrorCategory::invalid_result, "quadrature has no future event"});
}
} // namespace

template <AutocallableProduct Note>
Result<double> QuadratureAutocallableEngine::price(const Note& note, const PricingContext& context) const
{
    return integrate_autocallable(note, context, settings_);
}

template <AutocallableProduct Note>
Result<PricingResult> QuadratureAutocallableEngine::price_with_greeks(const Note& note, const PricingContext& context,
                                                                      GreeksRequest greeks, NumericalShiftSettings settings) const
{
    return detail::price_with_greeks(*this, note, context, greeks, settings);
}

template KIYOSI_EXPORT Result<double> QuadratureAutocallableEngine::price(const PhoenixOption&, const PricingContext&) const;
template KIYOSI_EXPORT Result<double> QuadratureAutocallableEngine::price(const SnowballOption&, const PricingContext&) const;
template KIYOSI_EXPORT Result<double> QuadratureAutocallableEngine::price(const BinarySnowballOption&, const PricingContext&) const;
template KIYOSI_EXPORT Result<double> QuadratureAutocallableEngine::price(const TernarySnowballOption&, const PricingContext&) const;
template KIYOSI_EXPORT Result<PricingResult> QuadratureAutocallableEngine::price_with_greeks(const PhoenixOption&, const PricingContext&, GreeksRequest, NumericalShiftSettings) const;
template KIYOSI_EXPORT Result<PricingResult> QuadratureAutocallableEngine::price_with_greeks(const SnowballOption&, const PricingContext&, GreeksRequest, NumericalShiftSettings) const;
template KIYOSI_EXPORT Result<PricingResult> QuadratureAutocallableEngine::price_with_greeks(const BinarySnowballOption&, const PricingContext&, GreeksRequest, NumericalShiftSettings) const;
template KIYOSI_EXPORT Result<PricingResult> QuadratureAutocallableEngine::price_with_greeks(const TernarySnowballOption&, const PricingContext&, GreeksRequest, NumericalShiftSettings) const;
} // namespace kiyosi
