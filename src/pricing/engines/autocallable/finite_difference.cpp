#include <kiyosi/pricing/engines/autocallable/finite_difference.hpp>

#include "../../detail/numerical_greeks.hpp"
#include <kiyosi/pricing/implied.hpp>

#include <algorithm>
#include <cmath>
#include <optional>
#include <utility>
#include <vector>

#include <kiyosi/market/schedule.hpp>
#include <kiyosi/pricing/detail/autocallable_traits.hpp>

#include "../../detail/calendar_dates.hpp"
#include "../../detail/autocallable_pricer.hpp"
#include "../../detail/fd_concentrated_grid.hpp"
#include "../../detail/fd_damping.hpp"
#include "../../detail/fd_event.hpp"
#include "../../detail/fd_grid.hpp"
#include "../../detail/fd_scheme.hpp"
#include "../../detail/math.hpp"

namespace kiyosi {
using namespace detail;

namespace {

struct ObservationEvent {
    double time;
    std::size_t index;
    AutocallableEvent event{};
};

// Reuse across coupon or market changes at fixed spot, valuation time and settings.
struct AutocallableFiniteDifferenceWorkspace {
    double contract_level{};
    std::optional<SpatialGrid> space;
    std::optional<LinearBoundaryStepper> stepper;
    std::optional<bool> daily_grid;
    FiniteDifferenceTimeGrid grid;
    std::vector<ObservationEvent> observation_events;
    std::vector<double> trading_times;
    std::vector<double> assets, alive, next_alive, knocked_in, next_knocked_in, curvature;
};

template <AutocallableProduct Note>
struct AutocallableFiniteDifferenceRequest {
    // Coupon trials use core-validated replacements with the bound contract's schedule.
    AutocallableContract<Note> contract;
    AutocallableFiniteDifferenceWorkspace workspace;
};

Result<PricingResult> rollback_autocallable(
    const AutocallableProgram& program, const AutocallableInitialState& initial,
    const PricingContext& context, const FiniteDifferenceSettings& settings,
    GreeksRequest output, double spot_shift, AutocallableFiniteDifferenceWorkspace& workspace)
{
    const double spot = context.spot_price();
    const auto& space = *workspace.space; // NOLINT(bugprone-unchecked-optional-access): price_autocallable initializes this request-owned grid.
    const auto& grid = workspace.grid;
    const auto& observation_events = workspace.observation_events;
    const auto& trading_times = workspace.trading_times;
    const double time_to_expiry = grid.times.back();
    const bool monitors_daily = program.daily_knock_in;
    const bool needs_knock_in_layer = monitors_daily && !initial.path.knocked_in &&
                                      program_knock_in_affects_payoff(program);
    const double rate = context.model_parameters().risk_free_rate();
    const double dividend = context.model_parameters().dividend_yield();
    const double sigma = context.model_parameters().volatility();
    const DiffusionParameters parameters{rate, dividend, sigma, scheme_theta(settings.scheme), space.sinh_scale == 0.0 ? 0.0 : space.spacing, space.nodes.empty() ? nullptr : &space};
    if (auto stable = check_explicit_stability(settings.scheme, grid, parameters, space.asset_step_count);
        !stable)
        return std::unexpected(stable.error());

    const auto event_at = [&](double time) {
        const auto found = std::ranges::lower_bound(observation_events, time, {}, &ObservationEvent::time);
        return found == observation_events.end() || found->time != time
                   ? AutocallableEvent{}
                   : found->event;
    };

    const std::size_t size = space.size();
    const auto& assets = workspace.assets;
    auto& alive = workspace.alive;
    auto& next_alive = workspace.next_alive;
    auto& knocked_in = workspace.knocked_in;
    auto& next_knocked_in = workspace.next_knocked_in;
    alive.resize(size);
    next_alive.resize(size);
    if (needs_knock_in_layer) {
        knocked_in.resize(size);
        next_knocked_in.resize(size);
    }
    std::vector<double> breaks;
    breaks.reserve(5);
    const auto breaks_at = [&](const AutocallableEvent& event, bool daily, bool terminal) -> const std::vector<double>& {
        breaks.clear();
        const auto add = [&](double value) {
            if (value > 0.0 && value < space.upper) breaks.push_back(space.coordinate_of(value));
        };
        if (program.has_knock_in && (terminal || (daily && needs_knock_in_layer))) add(program.knock_in_level);
        if (event.active) {
            add(event.knock_out_level);
            if (event.conditional_coupon && event.coupon != 0.0) add(event.coupon_barrier);
        }
        if (terminal && program.terminal_kind == AutocallableTerminalKind::downside_if_knocked_in) {
            add(program.lower_strike);
            add(program.upper_strike);
        }
        std::ranges::sort(breaks);
        breaks.erase(std::ranges::unique(breaks).begin(), breaks.end());
        return breaks;
    };
    const auto event_value = [&](const AutocallableEvent& event, bool daily, bool terminal,
                                 double asset, double continuation, double continuation_in) {
        if (event.active && program_knocked_out(event, asset)) {
            const double payment = program.principal_ratio + program_observation_coupon(event, asset);
            return std::array{payment, payment};
        }
        const double coupon = event.active && program.carries_observation_coupon
                                  ? program_observation_coupon(event, asset)
                                  : 0.0;
        if (terminal) {
            const bool ki = program_knocked_in(program, asset, initial.path.knocked_in, true);
            return std::array{program_terminal_settlement(program, asset, ki) + coupon,
                              program_terminal_settlement(program, asset, true) + coupon};
        }
        const bool transitioned = needs_knock_in_layer && daily &&
                                  program_knocked_in(program, asset, false, false);
        return std::array{(transitioned ? continuation_in : continuation) + coupon,
                          continuation_in + coupon};
    };
    const auto expiry_event = event_at(time_to_expiry);
    const auto& terminal_breaks = breaks_at(expiry_event, true, true);
    for (std::size_t index = 0; index < size; ++index) {
        const auto projected = project_finite_difference_event<2>(space, terminal_breaks, index, [&](double coordinate) {
            return event_value(expiry_event, true, true, space.asset_of_coordinate(coordinate), 0.0, 0.0);
        });
        alive[index] = projected[0];
        if (needs_knock_in_layer) knocked_in[index] = projected[1];
    }

    if (!workspace.stepper)
        workspace.stepper.emplace(size, space.upper, assets.back() - assets[size - 2], parameters);
    auto& stepper = *workspace.stepper;
    RannacherDamping damping{2};
    for (std::size_t step = grid.time_steps.size(); step-- > 0;) {
        const double dt = grid.time_steps[step];
        const auto substeps = damping.next(parameters.theta);
        auto step_parameters = parameters;
        step_parameters.theta = substeps.theta;
        stepper.set_parameters(step_parameters);
        for (int substep = 0; substep < substeps.count; ++substep) {
            const double width = dt / substeps.count;
            const bool advanced = needs_knock_in_layer
                                      ? stepper.advance_pair(knocked_in, next_knocked_in, alive, next_alive, width)
                                      : stepper.advance(alive, next_alive, width);
            if (!advanced)
                return std::unexpected(Error{ErrorCategory::invalid_result,
                                             "finite-difference system is numerically unstable"});
            alive.swap(next_alive);
            if (needs_knock_in_layer) knocked_in.swap(next_knocked_in);
        }
        const auto event = event_at(grid.times[step]);
        const bool daily = monitors_daily &&
                           std::ranges::binary_search(trading_times, grid.times[step]);
        if (event.active || (needs_knock_in_layer && daily)) {
            const auto& event_breaks = breaks_at(event, daily, false);
            const auto project_at = [&](std::size_t index) {
                return project_finite_difference_event<2>(space, event_breaks, index, [&](double coordinate) {
                    const double asset = space.asset_of_coordinate(coordinate);
                    const double continuation = space.linear_interpolate(alive, asset);
                    const double continuation_in = needs_knock_in_layer ? space.linear_interpolate(knocked_in, asset) : 0.0;
                    const auto value = event_value(event, daily, false, asset, continuation, continuation_in);
                    return std::array{value[0] - continuation, value[1] - continuation_in};
                });
            };
            if (!event.active) {
                // Daily knock-in replaces a monotone prefix; project the cut before mutating it.
                const std::size_t index = space.nearest_node(program.knock_in_level);
                const double projected = alive[index] + project_at(index)[0];
                const auto end = std::ranges::partition_point(assets, [&](double asset) {
                    return program_knocked_in(program, asset, false, false);
                });
                std::copy_n(knocked_in.begin(), end - assets.begin(), alive.begin());
                alive[index] = projected;
                damping.restart();
                continue;
            }
            for (std::size_t index = 0; index < size; ++index) {
                const auto value = event_value(event, daily, false, assets[index], alive[index], needs_knock_in_layer ? knocked_in[index] : 0.0);
                next_alive[index] = value[0];
                if (needs_knock_in_layer) next_knocked_in[index] = value[1];
            }
            std::optional<std::size_t> previous;
            for (const double coordinate_break : event_breaks) {
                const std::size_t index = space.nearest_node(space.asset_of_coordinate(coordinate_break));
                if (previous == index) continue;
                previous = index;
                const auto correction = project_at(index);
                next_alive[index] = alive[index] + correction[0];
                if (needs_knock_in_layer) next_knocked_in[index] = knocked_in[index] + correction[1];
            }
            alive.swap(next_alive);
            if (needs_knock_in_layer) knocked_in.swap(next_knocked_in);
            damping.restart();
        }
    }

    auto& curvature = workspace.curvature;
    curvature.clear();
    const double value = initial.path.coupons + space.interpolate(alive, spot, curvature);
    return finite_difference_result(value, spot, space.upper, output, spot_shift, [&](double shifted_spot) { return checked_price(initial.path.coupons + space.interpolate(alive, shifted_spot, curvature)); }, [&](double shifted_spot) { return space.delta(alive, shifted_spot, curvature); }, [&](double shifted_spot) { return space.gamma(alive, shifted_spot, curvature); });
}

template <AutocallableProduct Note>
Result<PricingResult> price_autocallable(
    const Note& note, const PricingContext& context, const FiniteDifferenceSettings& settings,
    AutocallableFiniteDifferenceRequest<Note>& request, GreeksRequest output, double spot_shift)
{
    const auto valid = request.contract.validate(context);
    if (!valid) return std::unexpected(valid.error());

    auto& workspace = request.workspace;
    const auto program = autocallable_program(note);
    const auto initial = autocallable_initial_state(note, context, program);

    if (initial.settlement) return make_pricing_result(*initial.settlement);
    if (const auto fixed = autocallable_fixed_value(note, context, program, initial))
        return make_pricing_result(*fixed,
                                   {{Greek::delta, output.has(Greek::delta) ? std::optional{0.0} : std::nullopt},
                                    {Greek::gamma, output.has(Greek::gamma) ? std::optional{0.0} : std::nullopt}});

    const Timestamp valuation = context.valuation_time();

    const bool monitors_daily = program.daily_knock_in;
    const bool needs_knock_in_layer = monitors_daily && !initial.path.knocked_in &&
                                      program_knock_in_affects_payoff(program);
    // Explicit-Euler stability can require daily subdivisions even without a knock-in layer.
    const bool daily_grid = monitors_daily && (needs_knock_in_layer || settings.scheme == FiniteDifferenceScheme::explicit_euler);
    auto& observation_events = workspace.observation_events;
    auto& trading_times = workspace.trading_times;
    auto& grid = workspace.grid;
    // Coupon shifts can remove knock-in exposure and change the required time grid.
    if (workspace.daily_grid != daily_grid) {
        std::vector<Date> anchors;
        observation_events.clear();
        const auto observations = note.observations_view();
        observation_events.reserve(observations.size());
        for (std::size_t index = 0; index < observations.size(); ++index) {
            const Date value = observations[index].date;
            if (value <= valuation) continue;
            const double time = actual_365_fixed_year_fraction(valuation, value);
            observation_events.push_back({time, index});
            anchors.push_back(value);
        }
        trading_times.clear();
        if (daily_grid) {
            const auto future_trading_dates = trading_dates(context.calendar(), valuation, note.expiry_date());
            trading_times.reserve(future_trading_dates.size());
            for (const Date value : future_trading_dates) {
                const double time = actual_365_fixed_year_fraction(valuation, value);
                trading_times.push_back(time);
                anchors.push_back(value);
            }
        }
        grid = make_finite_difference_time_grid(valuation, note.expiry_date(), settings.time_step_count, std::move(anchors));
        double nearest = grid.times.back();
        if (!observation_events.empty()) nearest = std::min(nearest, observation_events.front().time);
        if (!trading_times.empty()) nearest = std::min(nearest, trading_times.front());
        const int near_event_steps = daily_grid ? 64 : 128;
        // The global year step budget must still resolve an observation one hour from valuation.
        refine_initial_finite_difference_time_grid(grid, nearest, near_event_steps);
        workspace.daily_grid = daily_grid;
    }

    for (auto& observation : workspace.observation_events)
        observation.event = autocallable_event(note, observation.index);

    if (!workspace.space || !settings.asset_upper_boundary) {
        if (workspace.contract_level == 0.0)
            workspace.contract_level = highest_finite_difference_level(note);
        const double relevant = std::max(context.spot_price(), workspace.contract_level);
        const double upper = settings.asset_upper_boundary ? *settings.asset_upper_boundary
                                                           : volatility_adjusted_upper_boundary(relevant, note.expiry_date(), context);
        auto space = make_spatial_grid(settings, upper, {relevant}, upper > 4.0 * workspace.contract_level ? workspace.contract_level : 0.0);
        if (!space) return std::unexpected(space.error());
        if (settings.scheme != FiniteDifferenceScheme::explicit_euler) {
            std::vector<double> levels{program.initial_spot, program.knock_in_level, program.lower_strike, program.upper_strike};
            for (const auto& observation : workspace.observation_events) {
                levels.push_back(observation.event.knock_out_level);
                if (observation.event.conditional_coupon) levels.push_back(observation.event.coupon_barrier);
            }
            double nearest = grid.times.back();
            if (!observation_events.empty()) nearest = std::min(nearest, observation_events.front().time);
            if (!trading_times.empty()) nearest = std::min(nearest, trading_times.front());
            const double days = std::min(1.0, nearest * 365.0);
            // Follow diffusion width near events without introducing nearly coincident nodes.
            const double scale = workspace.contract_level * std::max(0.001 * std::sqrt(days), 64.0 * std::numeric_limits<double>::epsilon());
            *space = concentrate_spatial_grid(std::move(*space), std::move(levels), scale);
        }
        if (!workspace.space || workspace.space->upper != space->upper || workspace.space->sinh_scale != space->sinh_scale || workspace.space->nodes != space->nodes) {
            workspace.stepper.reset();
            workspace.assets.resize(space->size());
            for (std::size_t index = 0; index < workspace.assets.size(); ++index)
                workspace.assets[index] = space->asset_at(index);
        }
        workspace.space = std::move(*space);
    }
    return rollback_autocallable(program, initial, context, settings, output, spot_shift, workspace);
}

template <typename Note, typename ReplaceCoupon>
Result<double> solve_coupon(
    const FiniteDifferenceAutocallableEngine& engine, const Note& note, const PricingContext& context,
    double observed_price, ImpliedCouponSettings settings, const ReplaceCoupon& replace)
{
    AutocallableFiniteDifferenceRequest<Note> request{{note, context.calendar()}, {}};
    return detail::solve_implied_coupon(engine, note, context, observed_price, settings, replace,
                                        [&](const auto& scalar, const Note& instrument, const PricingContext& market) {
                                            return detail::price_value(price_autocallable(
                                                instrument, market, scalar.settings(), request, GreeksRequest{},
                                                NumericalShiftSettings{}.spot_shift));
                                        });
}

} // namespace

template <AutocallableProduct Note>
Result<double> implied_volatility(
    const FiniteDifferenceAutocallableEngine& engine, const Note& option, const PricingContext& context,
    double observed_price, ImpliedVolatilitySettings settings)
{
    AutocallableFiniteDifferenceRequest<Note> request{{option, context.calendar()}, {}};
    return detail::solve_implied_volatility(engine, option, context, observed_price, settings,
                                            [&](const auto& scalar, const Note& instrument, const PricingContext& market) {
                                                return detail::price_value(price_autocallable(
                                                    instrument, market, scalar.settings(), request, GreeksRequest{},
                                                    NumericalShiftSettings{}.spot_shift));
                                            });
}

template <AutocallableProduct Note>
Result<double> implied_coupon(
    const FiniteDifferenceAutocallableEngine& engine, const Note& option, const PricingContext& context,
    double observed_price, CouponQuoteConvention convention, ImpliedCouponSettings settings)
    requires requires(const Note& value, double coupon) {
        detail::replace_coupon(value, coupon, CouponQuoteConvention::preserve_maturity_coupon);
    }
{
    return solve_coupon(engine, option, context, observed_price, settings, [convention](const auto& note, double coupon) {
        return detail::replace_coupon(note, coupon, convention);
    });
}

Result<double> implied_coupon(
    const FiniteDifferenceAutocallableEngine& engine, const PhoenixOption& option, const PricingContext& context,
    double observed_price, ImpliedCouponSettings settings)
{
    return solve_coupon(engine, option, context, observed_price, settings, [](const auto& note, double coupon) {
        return detail::replace_coupon(note, coupon);
    });
}

template <AutocallableProduct Note>
Result<double> FiniteDifferenceAutocallableEngine::price(
    const Note& note, const PricingContext& context) const
{
    AutocallableFiniteDifferenceRequest<Note> request{{note, context.calendar()}, {}};
    return detail::price_value(price_autocallable(note, context, settings_, request, GreeksRequest{}, NumericalShiftSettings{}.spot_shift));
}

template <AutocallableProduct Note>
Result<PricingResult> FiniteDifferenceAutocallableEngine::price_with_greeks(const Note& option, const PricingContext& context,
                                                                            GreeksRequest greeks, NumericalShiftSettings settings) const
{
    AutocallableFiniteDifferenceRequest<Note> request{{option, context.calendar()}, {}};
    auto scenario = std::pair{context.valuation_time(), context.spot_price()};
    return detail::price_with_greeks(*this, option, context, greeks, settings,
                                     [&](const auto& engine, const PricingContext& shifted, GreeksRequest requested_output) {
                                         const auto next_scenario = std::pair{shifted.valuation_time(), shifted.spot_price()};
                                         if (scenario != next_scenario) {
                                             request.workspace = {};
                                             scenario = next_scenario;
                                         }
                                         return price_autocallable(option, shifted, engine.settings(), request, requested_output, settings.spot_shift);
                                     });
}

template KIYOSI_EXPORT Result<double> implied_volatility(
    const FiniteDifferenceAutocallableEngine&, const PhoenixOption&,
    const PricingContext&, double, ImpliedVolatilitySettings);
template KIYOSI_EXPORT Result<double> implied_volatility(
    const FiniteDifferenceAutocallableEngine&, const SnowballOption&,
    const PricingContext&, double, ImpliedVolatilitySettings);
template KIYOSI_EXPORT Result<double> implied_volatility(
    const FiniteDifferenceAutocallableEngine&, const BinarySnowballOption&,
    const PricingContext&, double, ImpliedVolatilitySettings);
template KIYOSI_EXPORT Result<double> implied_volatility(
    const FiniteDifferenceAutocallableEngine&, const TernarySnowballOption&,
    const PricingContext&, double, ImpliedVolatilitySettings);
template KIYOSI_EXPORT Result<double> implied_coupon(
    const FiniteDifferenceAutocallableEngine&, const SnowballOption&,
    const PricingContext&, double, CouponQuoteConvention, ImpliedCouponSettings);
template KIYOSI_EXPORT Result<double> implied_coupon(
    const FiniteDifferenceAutocallableEngine&, const BinarySnowballOption&,
    const PricingContext&, double, CouponQuoteConvention, ImpliedCouponSettings);
template KIYOSI_EXPORT Result<double> implied_coupon(
    const FiniteDifferenceAutocallableEngine&, const TernarySnowballOption&,
    const PricingContext&, double, CouponQuoteConvention, ImpliedCouponSettings);

template KIYOSI_EXPORT Result<double> FiniteDifferenceAutocallableEngine::price(const PhoenixOption&, const PricingContext&) const;
template KIYOSI_EXPORT Result<PricingResult> FiniteDifferenceAutocallableEngine::price_with_greeks(const PhoenixOption&, const PricingContext&, GreeksRequest, NumericalShiftSettings) const;
template KIYOSI_EXPORT Result<double> FiniteDifferenceAutocallableEngine::price(const SnowballOption&, const PricingContext&) const;
template KIYOSI_EXPORT Result<PricingResult> FiniteDifferenceAutocallableEngine::price_with_greeks(const SnowballOption&, const PricingContext&, GreeksRequest, NumericalShiftSettings) const;
template KIYOSI_EXPORT Result<double> FiniteDifferenceAutocallableEngine::price(const BinarySnowballOption&, const PricingContext&) const;
template KIYOSI_EXPORT Result<PricingResult> FiniteDifferenceAutocallableEngine::price_with_greeks(const BinarySnowballOption&, const PricingContext&, GreeksRequest, NumericalShiftSettings) const;
template KIYOSI_EXPORT Result<double> FiniteDifferenceAutocallableEngine::price(const TernarySnowballOption&, const PricingContext&) const;
template KIYOSI_EXPORT Result<PricingResult> FiniteDifferenceAutocallableEngine::price_with_greeks(const TernarySnowballOption&, const PricingContext&, GreeksRequest, NumericalShiftSettings) const;

} // namespace kiyosi
