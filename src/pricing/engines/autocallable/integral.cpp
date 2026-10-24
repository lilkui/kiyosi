#include <kiyosi/pricing/engines/autocallable/integral.hpp>
#include <kiyosi/pricing/implied.hpp>

#include <algorithm>
#include <array>
#include <cmath>
#include <limits>
#include <map>
#include <numeric>
#include <optional>
#include <tuple>
#include <utility>
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

using Scenario = std::tuple<Timestamp, BlackScholesMertonParameters, bool, std::size_t>;

struct PreparedCurve {
    Scenario scenario;
    std::vector<double> nodes;
    Curve continuation;
    double anchor;
    double drift;
    double deviation;
    double discount;

    double evaluate(double spot) const
    {
        const double current = log_price_ratio(spot, anchor);
        const auto [first, last] = active_cells(nodes, current + drift, deviation);
        std::vector<IntervalMoments> weights;
        weights.reserve(last - first);
        for (std::size_t cell = first; cell < last; ++cell)
            weights.push_back(cell_moments(nodes[cell], nodes[cell + 1], current + drift, deviation));
        return integrate_curve(continuation, weights,
                               {log_normal_cdf((nodes.front() - current - drift) / deviation),
                                log_normal_cdf((current + drift - nodes.back()) / deviation)},
                               discount, first);
    }
};

// Coupon trials keep the market, event levels and integration domain fixed.
struct CouponWorkspace {
    std::vector<double> nodes;
    std::vector<double> assets;
    CubicInterpolation interpolation;
    std::map<double, Transition> transitions;
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

void apply_event(const AutocallableProgram& program, const AutocallableEvent& event,
                 const std::vector<double>& assets, Curve& alive, const Curve& knocked_in,
                 bool transition)
{
    const auto limits = event_limits(program, event, alive);
    alive.low = transition && program_knocked_in(program, 0.0, false, false) ? event_limits(program, event, knocked_in)[0] : limits[0];
    alive.high = limits[1];
    for (std::size_t i = 0; i < alive.cells.size(); ++i) {
        const double asset = assets[i];
        auto& polynomial = alive.cells[i];
        if (event.active && program_knocked_out(event, asset)) {
            polynomial = {program.principal_ratio + program_observation_coupon(event, asset), 0.0, 0.0, 0.0};
        } else {
            const bool crossed = transition && program_knocked_in(program, asset, false, false);
            if (crossed) polynomial = knocked_in.cells[i];
            polynomial[0] += event_coupon(program, event, asset);
        }
    }
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
        if (!pieces.empty() && pieces.back().cash == cash && pieces.back().asset == weight)
            pieces.back().upper = edges[i + 1];
        else
            pieces.push_back({edges[i], edges[i + 1], cash, weight});
    }
    return pieces;
}

struct IntegrationState {
    AutocallableProgram program{};
    AutocallableInitialState initial{};
    std::optional<double> settled;
    bool layers{};
    std::vector<Event> events;
    std::vector<TerminalPiece> terminal, terminal_in;
    Curve alive, knocked_in;
    std::vector<double> values, values_in;
};

template <AutocallableProduct Note, std::size_t count>
Result<std::array<double, count>> integrate_autocallables(
    const std::array<const Note*, count>& notes, const PricingContext& context,
    const QuadratureSettings& settings, const std::array<std::optional<PreparedCurve>*, count>& prepared,
    const std::array<AutocallableContract<Note>*, count>& contracts, CouponWorkspace* workspace)
{
    std::array<IntegrationState, count> states;
    for (std::size_t index = 0; index < count; ++index) {
        const auto valid = contracts[index]->validate(context);
        if (!valid) return std::unexpected(valid.error());
        auto& state = states[index];
        state.program = autocallable_program(*notes[index]);
        state.initial = autocallable_initial_state(*notes[index], context, state.program);
        const Scenario scenario{context.valuation_time(), context.model_parameters(), state.initial.path.knocked_in, state.initial.next_observation};
        state.settled = state.initial.settlement;
        if (!state.settled) state.settled = autocallable_fixed_value(*notes[index], context, state.program, state.initial);
        if (!state.settled && *prepared[index] && (*prepared[index])->scenario == scenario)
            state.settled = state.initial.path.coupons + (*prepared[index])->evaluate(context.spot_price());
        state.layers = state.program.daily_knock_in && !state.initial.path.knocked_in && program_knock_in_affects_payoff(state.program);
    }
    std::array<double, count> prices{};
    // Coupon endpoints can have different fixed cashflows or daily knock-in grids.
    if (std::ranges::any_of(states, [](const auto& state) { return state.settled.has_value(); }) ||
        std::ranges::any_of(states, [&](const auto& state) { return state.layers != states.front().layers; })) {
        for (std::size_t index = 0; index < count; ++index) {
            const auto value = [&]() -> Result<double> {
                if (states[index].settled) return checked_price(*states[index].settled);
                const auto single = integrate_autocallables(std::array{notes[index]}, context, settings,
                                                            std::array{prepared[index]}, std::array{contracts[index]}, workspace);
                if (!single) return std::unexpected(single.error());
                return (*single)[0];
            }();
            if (!value) return std::unexpected(value.error());
            prices[index] = *value;
        }
        return prices;
    }
    const bool layers = states.front().layers;
    const auto& first = states.front();
    const auto dates = autocallable_event_dates(*notes.front(), context, first.initial.next_observation, layers);
    for (std::size_t index = 0; index < count; ++index) {
        auto& state = states[index];
        const auto observations = notes[index]->observations_view();
        std::size_t observation = state.initial.next_observation;
        for (const auto date : dates) {
            AutocallableEvent event{};
            if (observation < observations.size() && observations[observation].date == date)
                event = autocallable_event(*notes[index], observation++);
            state.events.push_back({date, actual_365_fixed_year_fraction(context.valuation_time(), date), event});
        }
    }
    const auto observations = notes.front()->observations_view();
    const double anchor = first.program.initial_spot > 0.0 ? first.program.initial_spot : observations.empty() ? 1.0
                                                                                                               : observations.front().knock_out_level;
    const double rate = context.model_parameters().risk_free_rate();
    const double sigma = context.model_parameters().volatility();
    const double drift_rate = rate - context.model_parameters().dividend_yield() - 0.5 * sigma * sigma;
    std::vector<double> breaks;
    const auto add = [&](double level) { if (level > 0.0) breaks.push_back(log_price_ratio(level, anchor)); };
    if (first.program.has_knock_in) add(first.program.knock_in_level);
    if (first.program.terminal_kind == AutocallableTerminalKind::downside_if_knocked_in) {
        add(first.program.lower_strike);
        add(first.program.upper_strike);
    }
    for (const auto& event : first.events)
        if (event.terms.active) {
            add(event.terms.knock_out_level);
            if (event.terms.conditional_coupon) add(event.terms.coupon_barrier);
        }
    std::ranges::sort(breaks);
    breaks.erase(std::ranges::unique(breaks).begin(), breaks.end());
    for (auto& state : states) {
        state.terminal = terminal_pieces(state.program, state.events.back().terms, breaks, anchor, state.initial.path.knocked_in);
        if (layers) state.terminal_in = terminal_pieces(state.program, state.events.back().terms, breaks, anchor, true);
    }
    const double current = log_price_ratio(context.spot_price(), anchor);
    if (first.events.size() == 1) {
        const double time = first.events.front().time;
        for (std::size_t index = 0; index < count; ++index) {
            const auto& state = states[index];
            const auto value = checked_price(state.initial.path.coupons + integrate_terminal(state.terminal, current, anchor, drift_rate * time, sigma * std::sqrt(time), -rate * time));
            if (!value) return std::unexpected(value.error());
            prices[index] = *value;
        }
        return prices;
    }
    const double horizon = first.events.back().time;
    const double extent = settings.integration_standard_deviations * sigma * std::sqrt(horizon);
    const double lower = std::min(0.0, breaks.empty() ? 0.0 : breaks.front()) + std::min(0.0, drift_rate * horizon) - extent;
    const double upper = std::max(0.0, breaks.empty() ? 0.0 : breaks.back()) + std::max(0.0, drift_rate * horizon) + extent;
    CouponWorkspace local;
    auto& storage = workspace ? *workspace : local;
    if (storage.nodes.empty()) {
        auto grid = integration_nodes(lower, upper, settings.asset_step_count, breaks);
        if (!grid) return std::unexpected(grid.error());
        storage.nodes = std::move(*grid);
        storage.assets = cell_assets(storage.nodes, anchor);
        storage.interpolation = CubicInterpolation{storage.nodes};
    }
    auto& nodes = storage.nodes;
    for (auto& state : states) {
        state.values.resize(nodes.size());
        if (layers) state.values_in.resize(nodes.size());
    }
    for (std::size_t event_index = first.events.size(); event_index-- > 0;) {
        const auto& event = first.events[event_index];
        const bool final = event_index + 1 == first.events.size();
        if (!final)
            for (auto& state : states) {
                apply_event(state.program, state.events[event_index].terms, storage.assets, state.alive, state.knocked_in, layers);
                if (layers) apply_event(state.program, state.events[event_index].terms, storage.assets, state.knocked_in, state.knocked_in, false);
            }
        const double time = event_index == 0 ? event.time : actual_365_fixed_year_fraction(first.events[event_index - 1].date, event.date);
        const double drift = drift_rate * time;
        const double deviation = sigma * std::sqrt(time);
        const double discount = -rate * time;
        if (!(deviation > 0.0) || !std::isfinite(deviation) || !std::isfinite(drift))
            return std::unexpected(Error{ErrorCategory::invalid_result, "quadrature transition is not representable"});
        if (event_index == 0) {
            for (std::size_t index = 0; index < count; ++index) {
                auto& state = states[index];
                auto grid = count == 1 && !workspace ? std::exchange(nodes, {}) : nodes;
                const Scenario scenario{context.valuation_time(), context.model_parameters(), state.initial.path.knocked_in, state.initial.next_observation};
                *prepared[index] = PreparedCurve{scenario, std::move(grid), std::move(state.alive), anchor, drift, deviation, discount};
                const auto value = checked_price(state.initial.path.coupons + (*prepared[index])->evaluate(context.spot_price()));
                if (!value) return std::unexpected(value.error());
                prices[index] = *value;
            }
            return prices;
        }
        if (!final) {
            auto found = storage.transitions.find(time);
            if (found == storage.transitions.end()) found = storage.transitions.emplace(time, transition_weights(nodes, drift, deviation)).first;
            const auto integrate = [&]<bool layered>() {
                constexpr auto curve_count = count * (layered ? 2 : 1);
                std::array<const Curve*, curve_count> curves{};
                std::array<std::span<double>, curve_count> values{};
                for (std::size_t index = 0; index < count; ++index) {
                    const auto offset = index * (layered ? 2 : 1);
                    curves[offset] = &states[index].alive;
                    values[offset] = states[index].values;
                    if constexpr (layered) {
                        curves[offset + 1] = &states[index].knocked_in;
                        values[offset + 1] = states[index].values_in;
                    }
                }
                if constexpr (curve_count == 1) integrate_curves(*curves[0], found->second, discount, values[0]);
                else integrate_curve_batch(curves, found->second, discount, values);
            };
            if (layers) integrate.template operator()<true>();
            else integrate.template operator()<false>();
        }
        for (auto& state : states) {
            for (std::size_t target = 0; target < nodes.size(); ++target) {
                if (final) {
                    state.values[target] = integrate_terminal(state.terminal, nodes[target], anchor, drift, deviation, discount);
                    if (layers) state.values_in[target] = integrate_terminal(state.terminal_in, nodes[target], anchor, drift, deviation, discount);
                }
                if (!std::isfinite(state.values[target]) || (layers && !std::isfinite(state.values_in[target])))
                    return std::unexpected(Error{ErrorCategory::invalid_result, "quadrature continuation is non-finite"});
            }
            const double low = final ? state.terminal.front().cash : state.alive.low;
            const double high = final ? state.terminal.back().cash : state.alive.high;
            storage.interpolation.interpolate(state.values, scaled_exponential(low, discount), scaled_exponential(high, discount), state.alive);
            if (layers) {
                const double low_in = final ? state.terminal_in.front().cash : state.knocked_in.low;
                const double high_in = final ? state.terminal_in.back().cash : state.knocked_in.high;
                storage.interpolation.interpolate(state.values_in, scaled_exponential(low_in, discount), scaled_exponential(high_in, discount), state.knocked_in);
            }
        }
    }
    return std::unexpected(Error{ErrorCategory::invalid_result, "quadrature has no future event"});
}

template <AutocallableProduct Note>
Result<double> integrate_autocallable(const Note& note, const PricingContext& context,
                                      const QuadratureSettings& settings, std::optional<PreparedCurve>& prepared,
                                      AutocallableContract<Note>& contract, CouponWorkspace* workspace = nullptr)
{
    const auto prices = integrate_autocallables(std::array{&note}, context, settings, std::array{&prepared}, std::array{&contract}, workspace);
    if (!prices) return std::unexpected(prices.error());
    return (*prices)[0];
}

template <typename Note, typename ReplaceCoupon>
Result<double> solve_coupon(
    const QuadratureAutocallableEngine& engine, const Note& note, const PricingContext& context,
    double observed_price, ImpliedCouponSettings settings, const ReplaceCoupon& replace)
{
    CouponWorkspace workspace;
    return detail::solve_implied_coupon(engine, note, context, observed_price, settings, replace, [&](const auto& scalar, const Note& instrument, const PricingContext& market) {
                                            std::optional<PreparedCurve> prepared;
                                            AutocallableContract<Note> contract{instrument, market.calendar()};
                                            return integrate_autocallable(instrument, market, scalar.settings(), prepared, contract, &workspace); }, [&](const auto& scalar, const Note& lower, const Note& upper, const PricingContext& market) {
                                            AutocallableContract<Note> lower_contract{lower, market.calendar()}, upper_contract{upper, market.calendar()};
                                            std::optional<PreparedCurve> lower_prepared, upper_prepared;
                                            return integrate_autocallables(std::array{&lower, &upper}, market, scalar.settings(),
                                                                          std::array{&lower_prepared, &upper_prepared}, std::array{&lower_contract, &upper_contract}, &workspace); });
}
} // namespace

template <AutocallableProduct Note>
Result<double> implied_volatility(
    const QuadratureAutocallableEngine& engine, const Note& option, const PricingContext& context,
    double observed_price, ImpliedVolatilitySettings settings)
{
    AutocallableContract<Note> contract{option, context.calendar()};
    return detail::solve_implied_volatility(engine, option, context, observed_price, settings,
                                            [&](const auto& scalar, const Note& instrument, const PricingContext& market) {
                                                std::optional<PreparedCurve> prepared;
                                                return integrate_autocallable(instrument, market, scalar.settings(), prepared, contract);
                                            });
}

template <AutocallableProduct Note>
Result<double> implied_coupon(
    const QuadratureAutocallableEngine& engine, const Note& option, const PricingContext& context,
    double observed_price, CouponQuoteConvention convention, ImpliedCouponSettings settings)
    requires requires(const Note& value, double coupon) {
        detail::replace_coupon(value, coupon, CouponQuoteConvention::preserve_maturity_coupon);
    }
{
    return solve_coupon(engine, option, context, observed_price, settings, [convention](const auto& instrument, double coupon) {
        return detail::replace_coupon(instrument, coupon, convention);
    });
}

Result<double> implied_coupon(
    const QuadratureAutocallableEngine& engine, const PhoenixOption& option, const PricingContext& context,
    double observed_price, ImpliedCouponSettings settings)
{
    return solve_coupon(engine, option, context, observed_price, settings, [](const auto& instrument, double coupon) {
        return detail::replace_coupon(instrument, coupon);
    });
}

template KIYOSI_EXPORT Result<double> implied_volatility(
    const QuadratureAutocallableEngine&, const PhoenixOption&, const PricingContext&, double, ImpliedVolatilitySettings);
template KIYOSI_EXPORT Result<double> implied_volatility(
    const QuadratureAutocallableEngine&, const SnowballOption&, const PricingContext&, double, ImpliedVolatilitySettings);
template KIYOSI_EXPORT Result<double> implied_volatility(
    const QuadratureAutocallableEngine&, const BinarySnowballOption&, const PricingContext&, double, ImpliedVolatilitySettings);
template KIYOSI_EXPORT Result<double> implied_volatility(
    const QuadratureAutocallableEngine&, const TernarySnowballOption&, const PricingContext&, double, ImpliedVolatilitySettings);

template KIYOSI_EXPORT Result<double> implied_coupon(
    const QuadratureAutocallableEngine&, const SnowballOption&, const PricingContext&, double,
    CouponQuoteConvention, ImpliedCouponSettings);
template KIYOSI_EXPORT Result<double> implied_coupon(
    const QuadratureAutocallableEngine&, const BinarySnowballOption&, const PricingContext&, double,
    CouponQuoteConvention, ImpliedCouponSettings);
template KIYOSI_EXPORT Result<double> implied_coupon(
    const QuadratureAutocallableEngine&, const TernarySnowballOption&, const PricingContext&, double,
    CouponQuoteConvention, ImpliedCouponSettings);

template <AutocallableProduct Note>
Result<double> QuadratureAutocallableEngine::price(const Note& note, const PricingContext& context) const
{
    std::optional<PreparedCurve> prepared;
    AutocallableContract<Note> contract{note, context.calendar()};
    return integrate_autocallable(note, context, settings_, prepared, contract);
}

template <AutocallableProduct Note>
Result<PricingResult> QuadratureAutocallableEngine::price_with_greeks(const Note& note, const PricingContext& context,
                                                                      GreeksRequest greeks, NumericalShiftSettings settings) const
{
    return detail::evaluate_greeks(*this, note, context, greeks, settings,
                                   [&](const auto& engine) -> Result<PricingResult> {
                                       std::optional<PreparedCurve> prepared;
                                       AutocallableContract<Note> contract{note, context.calendar()};
                                       const auto price = [&](const PricingContext& shifted) {
                                           return integrate_autocallable(note, shifted, engine.settings(), prepared, contract);
                                       };
                                       const auto value = price(context);
                                       if (!value) return std::unexpected(value.error());
                                       return detail::complete_greeks(price, note, context, greeks, settings, *make_pricing_result(*value));
                                   });
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
