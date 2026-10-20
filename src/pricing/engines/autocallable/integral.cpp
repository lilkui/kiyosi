#include <kiyosi/pricing/engines/autocallable/integral.hpp>

#include <algorithm>
#include <array>
#include <cmath>
#include <limits>
#include <map>
#include <numeric>
#include <vector>

#include <kiyosi/pricing/detail/autocallable_traits.hpp>
#include <kiyosi/pricing/detail/numerical_greeks.hpp>

#include "../../detail/autocallable_pricer.hpp"
#include "../../detail/calendar_dates.hpp"
#include "../../detail/math.hpp"

namespace kiyosi {
namespace {
using namespace detail;
using Polynomial = std::array<double, 4>;
constexpr double infinity = std::numeric_limits<double>::infinity();

double signed_exponential(double value, double exponent)
{
    if (value == 0.0) return 0.0;
    const double weight = std::exp(exponent);
    if (std::isnormal(weight) && std::isfinite(value * weight)) return value * weight;
    return std::copysign(std::exp(std::log(std::abs(value)) + exponent), value);
}

double log_interval_mass(double lower, double upper)
{
    if (lower >= upper) return -infinity;
    if (lower == -infinity) return log_normal_cdf(upper);
    if (upper == infinity) return log_normal_cdf(-lower);
    const double center = std::midpoint(lower, upper);
    const double half = 0.5 * (upper - lower);
    if (std::abs(center) * half + 0.5 * half * half < 0.25) {
        double integral = 0.0;
        double previous = 0.0;
        double term = 1.0;
        for (int order = 0; order < 24; ++order) {
            if (order % 2 == 0) integral += 2.0 * term / (order + 1.0);
            const double next = (-center * half * term - half * half * previous) / (order + 1.0);
            previous = term;
            term = next;
        }
        return -0.5 * center * center + std::log(inverse_sqrt_two_pi) + std::log(half) + std::log(integral);
    }
    const double high = lower >= 0.0 ? log_normal_cdf(-lower) : log_normal_cdf(upper);
    const double low = lower >= 0.0 ? log_normal_cdf(-upper) : log_normal_cdf(lower);
    return high + std::log(-std::expm1(low - high));
}

struct IntervalMoments {
    double mass{};
    double log_mass = -infinity;
    std::array<double, 3> conditional{};

    double integrate(const Polynomial& polynomial) const
    {
        if (log_mass < -1500.0) return 0.0;
        const double mean = polynomial[0] + polynomial[1] * conditional[0] +
                            polynomial[2] * conditional[1] + polynomial[3] * conditional[2];
        return mass > 0.0 ? mean * mass : signed_exponential(mean, log_mass);
    }
};

// Moments of u=(x-left)/(right-left), conditional on x lying in this cell.
IntervalMoments cell_moments(double left, double right, double mean, double deviation)
{
    const double width = right - left;
    const double center = (std::midpoint(left, right) - mean) / deviation;
    const double half = (0.5 * width) / deviation;
    IntervalMoments result;
    if (half > 0.0 && std::abs(center) * half + 0.5 * half * half < 0.25) {
        // A centered density series avoids subtracting almost equal CDFs and raw moments.
        std::array<double, 4> integrals{};
        double previous = 0.0;
        double term = 1.0;
        for (int order = 0; order < 24; ++order) {
            for (int power = 0; power < 4; ++power)
                if ((order + power) % 2 == 0)
                    integrals[static_cast<std::size_t>(power)] += 2.0 * term / (order + power + 1.0);
            const double next = (-center * half * term - half * half * previous) / (order + 1.0);
            previous = term;
            term = next;
        }
        const double first = integrals[1] / integrals[0];
        const double second = integrals[2] / integrals[0];
        const double third = integrals[3] / integrals[0];
        result.log_mass = -0.5 * center * center + std::log(inverse_sqrt_two_pi) +
                          std::log(half) + std::log(integrals[0]);
        result.conditional = {0.5 * (1.0 + first), 0.25 * (1.0 + 2.0 * first + second),
                              0.125 * (1.0 + 3.0 * first + 3.0 * second + third)};
    } else {
        const double lower = (left - mean) / deviation;
        const double upper = (right - mean) / deviation;
        result.log_mass = log_interval_mass(lower, upper);
        // Even a largest finite coefficient cannot recover a representable contribution here.
        if (result.log_mass < -1500.0) return result;
        const auto weighted_density = [&](double z, int order) {
            if (!std::isfinite(z)) return 0.0;
            const double density = std::exp(-0.5 * z * z + std::log(inverse_sqrt_two_pi) - result.log_mass);
            if (order == 0) return density;
            if (order == 1) return z * density;
            return (z * z + 2.0) * density;
        };
        const double first = weighted_density(lower, 0) - weighted_density(upper, 0);
        const double second = 1.0 + weighted_density(lower, 1) - weighted_density(upper, 1);
        const double third = weighted_density(lower, 2) - weighted_density(upper, 2);
        const double offset = (mean - left) / width;
        const double scale = deviation / width;
        result.conditional = {
            offset + scale * first,
            offset * offset + 2.0 * offset * scale * first + scale * scale * second,
            offset * offset * offset + 3.0 * offset * offset * scale * first +
                3.0 * offset * scale * scale * second + scale * scale * scale * third};
    }
    result.mass = std::exp(result.log_mass);
    return result;
}

struct Sum {
    double value{};
    double correction{};
    void add(double term)
    {
        const double next = value + term;
        correction += std::abs(value) >= std::abs(term) ? (value - next) + term : (term - next) + value;
        value = next;
    }
    double total() const { return value + correction; }
};

struct Event {
    Date date;
    double time;
    AutocallableEvent terms{};
};

struct Curve {
    std::vector<Polynomial> cells;
    double low{};
    double high{};
};

Curve interpolate(const std::vector<double>& nodes, const std::vector<double>& values, double low, double high)
{
    Curve curve{{}, low, high};
    curve.cells.reserve(nodes.size() - 1);
    for (std::size_t cell = 0; cell + 1 < nodes.size(); ++cell) {
        const std::size_t first = std::min(cell == 0 ? 0 : cell - 1, nodes.size() - 4);
        const double width = nodes[cell + 1] - nodes[cell];
        std::array<double, 4> coordinates{}, differences{};
        for (std::size_t k = 0; k < 4; ++k) {
            coordinates[k] = (nodes[first + k] - nodes[cell]) / width;
            differences[k] = values[first + k];
        }
        for (std::size_t order = 1; order < 4; ++order)
            for (std::size_t k = 4; k-- > order;)
                differences[k] = (differences[k] - differences[k - 1]) / (coordinates[k] - coordinates[k - order]);
        Polynomial polynomial{differences[3], 0.0, 0.0, 0.0};
        for (std::size_t k = 3; k-- > 0;) {
            Polynomial next{};
            for (std::size_t power = 0; power < 4; ++power) {
                next[power] -= coordinates[k] * polynomial[power];
                if (power != 0) next[power] += polynomial[power - 1];
            }
            next[0] += differences[k];
            polynomial = next;
        }
        curve.cells.push_back(polynomial);
    }
    return curve;
}

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

struct Transition {
    std::vector<IntervalMoments> weights;
    std::vector<std::array<double, 2>> tails;
};

Transition transition_weights(const std::vector<double>& nodes, double drift, double deviation)
{
    Transition transition;
    const std::size_t cells = nodes.size() - 1;
    transition.weights.reserve(nodes.size() * cells);
    transition.tails.reserve(nodes.size());
    for (const double node : nodes) {
        const double mean = node + drift;
        transition.tails.push_back({log_normal_cdf((nodes.front() - mean) / deviation),
                                    log_normal_cdf((mean - nodes.back()) / deviation)});
        for (std::size_t cell = 0; cell < cells; ++cell)
            transition.weights.push_back(cell_moments(nodes[cell], nodes[cell + 1], mean, deviation));
    }
    return transition;
}

double integrate_curve(const Curve& curve, std::span<const IntervalMoments> weights,
                       const std::array<double, 2>& tails, double log_discount)
{
    Sum sum;
    sum.add(signed_exponential(curve.low, tails[0] + log_discount));
    sum.add(signed_exponential(curve.high, tails[1] + log_discount));
    const double discount = std::exp(log_discount);
    for (std::size_t cell = 0; cell < curve.cells.size(); ++cell)
        sum.add(std::isnormal(discount) ? weights[cell].integrate(curve.cells[cell]) * discount : signed_exponential(weights[cell].integrate(curve.cells[cell]), log_discount));
    return sum.total();
}

// The terminal function is affine in S on each exact contractual interval.
struct TerminalPiece {
    double lower;
    double upper;
    double cash;
    double asset;
};

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

double integrate_terminal(const std::vector<TerminalPiece>& pieces, double log_spot, double anchor,
                          double drift, double deviation, double log_discount)
{
    Sum sum;
    const double mean = log_spot + drift;
    for (const auto& piece : pieces) {
        const double lower = (piece.lower - mean) / deviation;
        const double upper = (piece.upper - mean) / deviation;
        sum.add(signed_exponential(piece.cash, log_discount + log_interval_mass(lower, upper)));
        if (piece.asset != 0.0)
            sum.add(signed_exponential(piece.asset, std::log(anchor) + mean + 0.5 * deviation * deviation +
                                                        log_discount + log_interval_mass(lower - deviation, upper - deviation)));
    }
    return sum.total();
}

template <AutocallableProduct Note>
Result<double> integrate_autocallable(const Note& note, const PricingContext& context,
                                      const AutocallableQuadratureSettings& settings)
{
    AutocallableContract<Note> contract{note, context.calendar()};
    const auto valid = contract.validate(context);
    if (!valid) return std::unexpected(valid.error());
    const auto program = autocallable_program(note);
    const auto initial = autocallable_initial_state(note, context, program);
    if (initial.settlement) return checked_price(*initial.settlement);
    if (const auto fixed = autocallable_fixed_value(note, context, program, initial)) return checked_price(*fixed);
    const bool layers = program.daily_knock_in && !initial.path.knocked_in && program_knock_in_affects_payoff(program);
    std::vector<Date> dates = layers ? trading_dates(context.calendar(), context.valuation_time(), note.expiry_date()) : std::vector<Date>{};
    const auto observations = note.observations_view();
    for (std::size_t i = initial.next_observation; i < observations.size(); ++i)
        dates.push_back(observations[i].date);
    dates.push_back(note.expiry_date());
    std::ranges::sort(dates);
    dates.erase(std::ranges::unique(dates).begin(), dates.end());
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
    if (!std::isfinite(lower) || !std::isfinite(upper) || lower >= upper)
        return std::unexpected(Error{ErrorCategory::invalid_result, "quadrature integration domain is not representable"});
    std::vector<double> nodes;
    const auto steps = static_cast<std::size_t>(settings.asset_step_count);
    nodes.reserve(steps + 1 + breaks.size());
    for (std::size_t i = 0; i <= steps; ++i)
        nodes.push_back(std::lerp(lower, upper, static_cast<double>(i) / static_cast<double>(steps)));
    nodes.insert(nodes.end(), breaks.begin(), breaks.end());
    std::ranges::sort(nodes);
    nodes.erase(std::ranges::unique(nodes).begin(), nodes.end());
    if (nodes.size() < 4)
        return std::unexpected(Error{ErrorCategory::invalid_result, "quadrature grid has insufficient representable nodes"});
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
        }
        for (std::size_t target = 0; target < nodes.size(); ++target) {
            if (final) {
                values[target] = integrate_terminal(terminal, nodes[target], anchor, drift, deviation, discount);
                if (layers) values_in[target] = integrate_terminal(terminal_in, nodes[target], anchor, drift, deviation, discount);
            } else {
                const auto weights = std::span{transition->weights}.subspan(target * alive.cells.size(), alive.cells.size());
                values[target] = integrate_curve(alive, weights, transition->tails[target], discount);
                if (layers) values_in[target] = integrate_curve(knocked_in, weights, transition->tails[target], discount);
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
