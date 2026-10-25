#include <kiyosi/pricing/engines/autocallable/finite_difference.hpp>

#include "../../detail/numerical_greeks.hpp"
#include <kiyosi/pricing/implied.hpp>

#include <algorithm>
#include <array>
#include <cmath>
#include <functional>
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
    double requested_upper{};
    double concentration_scale{};
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

class AutocallableRollback {
public:
    AutocallableRollback(AutocallableProgram program, AutocallableInitialState initial,
                         const PricingContext& context, AutocallableFiniteDifferenceWorkspace& workspace)
        : program_(program), initial_(initial), context_(context), workspace_(workspace) { breaks_.reserve(5); }

    AutocallableFiniteDifferenceWorkspace& workspace() const { return workspace_.get(); }
    bool layered() const
    {
        return program_.daily_knock_in && !initial_.path.knocked_in && program_knock_in_affects_payoff(program_);
    }

    void initialize()
    {
        auto& workspace = workspace_.get();
        const auto& space = *workspace.space;
        const auto size = space.size();
        auto& alive = workspace.alive;
        auto& next_alive = workspace.next_alive;
        auto& knocked_in = workspace.knocked_in;
        auto& next_knocked_in = workspace.next_knocked_in;
        const bool needs_knock_in_layer = layered();
        const double time_to_expiry = workspace.grid.times.back();
        alive.resize(size);
        next_alive.resize(size);
        if (needs_knock_in_layer) {
            knocked_in.resize(size);
            next_knocked_in.resize(size);
        }
        const auto expiry_event = event_at(time_to_expiry);
        const auto& terminal_breaks = breaks_at(expiry_event, true, true);
        for (std::size_t index = 0; index < size; ++index) {
            const auto projected = project_finite_difference_event<2>(space, terminal_breaks, index, [&](double coordinate) {
                return event_value(expiry_event, true, true, space.asset_of_coordinate(coordinate), 0.0, 0.0);
            });
            alive[index] = projected[0];
            if (needs_knock_in_layer) knocked_in[index] = projected[1];
        }
    }

    bool apply_event(double time)
    {
        const auto& program = program_;
        const bool monitors_daily = program.daily_knock_in;
        const auto& trading_times = workspace_.get().trading_times;
        auto& workspace = workspace_.get();
        const auto& space = *workspace.space;
        const auto size = space.size();
        const auto& assets = workspace.assets;
        auto& alive = workspace.alive;
        auto& next_alive = workspace.next_alive;
        auto& knocked_in = workspace.knocked_in;
        auto& next_knocked_in = workspace.next_knocked_in;
        const bool needs_knock_in_layer = layered();
        const auto event = event_at(time);
        const bool daily = monitors_daily &&
                           std::ranges::binary_search(trading_times, time);
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
                return true;
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
            return true;
        }
        return false;
    }

    Result<PricingResult> result(GreeksRequest output, double spot_shift) const
    {
        auto& workspace = workspace_.get();
        const auto& space = *workspace.space;
        auto& alive = workspace.alive;
        const auto& initial = initial_;
        const double spot = context_.get().spot_price();
        auto& curvature = workspace.curvature;
        curvature.clear();
        const bool nonuniform = !space.nodes.empty();
        // Greek extraction visits only the center and one spot bump in each direction.
        std::array<std::optional<std::array<double, 3>>, 3> samples{};
        const auto sample_at = [&](double shifted_spot) {
            const std::size_t index = shifted_spot == spot ? 0 : shifted_spot > spot ? 1
                                                                                     : 2;
            auto& sample = samples[index];
            if (!sample) sample = space.sample(alive, shifted_spot, curvature);
            return *sample;
        };
        const auto price_at = [&](double shifted_spot) {
            return initial.path.coupons + (nonuniform ? sample_at(shifted_spot)[0] : space.interpolate(alive, shifted_spot, curvature));
        };
        const double value = price_at(spot);
        return finite_difference_result(value, spot, space.upper, output, spot_shift, [&](double shifted_spot) { return checked_price(price_at(shifted_spot)); }, [&](double shifted_spot) { return nonuniform ? sample_at(shifted_spot)[1] : space.delta(alive, shifted_spot, curvature); }, [&](double shifted_spot) { return nonuniform ? sample_at(shifted_spot)[2] : space.gamma(alive, shifted_spot, curvature); });
    }

private:
    AutocallableEvent event_at(double time) const
    {
        const auto& observation_events = workspace_.get().observation_events;
        const auto found = std::ranges::lower_bound(observation_events, time, {}, &ObservationEvent::time);
        return found == observation_events.end() || found->time != time
                   ? AutocallableEvent{}
                   : found->event;
    }

    const std::vector<double>& breaks_at(const AutocallableEvent& event, bool daily, bool terminal)
    {
        const auto& program = program_;
        const auto& space = *workspace_.get().space;
        const bool needs_knock_in_layer = layered();
        breaks_.clear();
        const auto add = [&](double value) {
            if (value > 0.0 && value < space.upper) breaks_.push_back(space.coordinate_of(value));
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
        std::ranges::sort(breaks_);
        breaks_.erase(std::ranges::unique(breaks_).begin(), breaks_.end());
        return breaks_;
    }

    std::array<double, 2> event_value(const AutocallableEvent& event, bool daily, bool terminal,
                                      double asset, double continuation, double continuation_in) const
    {
        const auto& program = program_;
        const auto& initial = initial_;
        const bool needs_knock_in_layer = layered();
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
    }

    AutocallableProgram program_;
    AutocallableInitialState initial_;
    std::reference_wrapper<const PricingContext> context_;
    std::reference_wrapper<AutocallableFiniteDifferenceWorkspace> workspace_;
    std::vector<double> breaks_;
};

template <std::size_t count>
Result<std::array<PricingResult, count>> rollback_autocallables(
    std::array<AutocallableRollback, count>& states, const PricingContext& context,
    const FiniteDifferenceSettings& settings, GreeksRequest output, double spot_shift)
{
    auto& workspace = states.front().workspace();
    const auto& space = *workspace.space;
    const auto& assets = workspace.assets;
    const auto& grid = workspace.grid;
    const auto parameters = diffusion_parameters(context, scheme_theta(settings.scheme),
                                                 space.sinh_scale == 0.0 ? 0.0 : space.spacing, space.nodes.empty() ? nullptr : &space);
    if (auto stable = check_curve_explicit_stability(settings.scheme, grid, parameters, space.asset_step_count, context); !stable)
        return std::unexpected(stable.error());
    for (auto& state : states)
        state.initialize();
    if (!workspace.stepper)
        workspace.stepper.emplace(space.size(), space.upper, assets.back() - assets[assets.size() - 2], parameters);
    auto& stepper = *workspace.stepper;
    RannacherDamping damping{2};
    for (std::size_t step = grid.time_steps.size(); step-- > 0;) {
        const auto substeps = damping.next(parameters.theta);
        auto step_parameters = parameters;
        step_parameters.theta = substeps.theta;
        for (int substep = 0; substep < substeps.count; ++substep) {
            const double dt = grid.time_steps[step] / substeps.count;
            const double time = grid.times[step] + dt * (substeps.count - substep - 1);
            stepper.set_parameters(diffusion_interval(step_parameters, context, time, time + dt));
            const auto advance = [&]<bool layered>() {
                constexpr auto layers = count * (layered ? 2 : 1);
                std::array<std::span<const double>, layers> old;
                std::array<std::span<double>, layers> next;
                for (std::size_t index = 0; index < count; ++index) {
                    auto& values = states[index].workspace();
                    const auto offset = index * (layered ? 2 : 1);
                    if constexpr (layered) {
                        old[offset] = values.knocked_in;
                        next[offset] = values.next_knocked_in;
                    }
                    old[offset + (layered ? 1 : 0)] = values.alive;
                    next[offset + (layered ? 1 : 0)] = values.next_alive;
                }
                return stepper.advance_many(old, next, grid.time_steps[step] / substeps.count);
            };
            const bool advanced = states.front().layered() ? advance.template operator()<true>() : advance.template operator()<false>();
            if (!advanced)
                return std::unexpected(Error{ErrorCategory::invalid_result, "finite-difference system is numerically unstable"});
            for (auto& state : states) {
                auto& values = state.workspace();
                values.alive.swap(values.next_alive);
                if (state.layered()) values.knocked_in.swap(values.next_knocked_in);
            }
        }
        bool applied = false;
        for (auto& state : states)
            applied = state.apply_event(grid.times[step]) || applied;
        if (applied) damping.restart();
    }
    const auto results = [&]<std::size_t... index>(std::index_sequence<index...>) {
        return std::array{states[index].result(output, spot_shift)...};
    }(std::make_index_sequence<count>{});
    for (const auto& result : results)
        if (!result) return std::unexpected(result.error());
    return [&]<std::size_t... index>(std::index_sequence<index...>) {
        return std::array{*results[index]...};
    }(std::make_index_sequence<count>{});
}

Result<PricingResult> rollback_autocallable(
    const AutocallableProgram& program, const AutocallableInitialState& initial,
    const PricingContext& context, const FiniteDifferenceSettings& settings,
    GreeksRequest output, double spot_shift, AutocallableFiniteDifferenceWorkspace& workspace)
{
    std::array states{AutocallableRollback{program, initial, context, workspace}};
    const auto result = rollback_autocallables(states, context, settings, output, spot_shift);
    if (!result) return std::unexpected(result.error());
    return (*result)[0];
}

struct PreparedAutocallable {
    AutocallableProgram program;
    AutocallableInitialState initial;
    std::optional<PricingResult> settled;
};

template <AutocallableProduct Note>
Result<PreparedAutocallable> prepare_autocallable(
    const Note& note, const PricingContext& context, const FiniteDifferenceSettings& settings,
    AutocallableFiniteDifferenceRequest<Note>& request, GreeksRequest output)
{
    const auto valid = request.contract.validate(context);
    if (!valid) return std::unexpected(valid.error());

    auto& workspace = request.workspace;
    const auto program = autocallable_program(note);
    const auto initial = autocallable_initial_state(note, context, program);

    const auto fixed = initial.settlement ? std::optional<double>{} : autocallable_fixed_value(note, context, program, initial);
    if (initial.settlement || fixed) {
        const auto value = initial.settlement ? make_pricing_result(*initial.settlement)
                                              : make_pricing_result(*fixed,
                                                                    {{Greek::delta, output.has(Greek::delta) ? std::optional{0.0} : std::nullopt},
                                                                     {Greek::gamma, output.has(Greek::gamma) ? std::optional{0.0} : std::nullopt}});
        if (!value) return std::unexpected(value.error());
        return PreparedAutocallable{program, initial, *value};
    }

    if (const auto covered = validate_curve_horizon(context, start_of_day(note.expiry_date())); !covered)
        return std::unexpected(covered.error());
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
        grid = curve_finite_difference_time_grid(context, note.expiry_date(), settings.time_step_count, std::move(anchors));
        double nearest = grid.times.back();
        if (!observation_events.empty()) nearest = std::min(nearest, observation_events.front().time);
        if (!trading_times.empty()) nearest = std::min(nearest, trading_times.front());
        const auto knots = curve_times(context, start_of_day(note.expiry_date()));
        if (!knots.empty()) nearest = std::min(nearest, knots.front());
        const int near_event_steps = daily_grid ? 64 : 128;
        // The global year step budget must still resolve an observation one hour from valuation.
        refine_initial_finite_difference_time_grid(grid, nearest, near_event_steps);
        workspace.daily_grid = daily_grid;
    }

    for (auto& observation : workspace.observation_events)
        observation.event = autocallable_event(note, observation.index);

    if (workspace.contract_level == 0.0)
        workspace.contract_level = highest_finite_difference_level(note);
    const double relevant = std::max(context.spot_price(), workspace.contract_level);
    const double upper = settings.asset_upper_boundary ? *settings.asset_upper_boundary
                                                       : volatility_adjusted_upper_boundary(relevant, note.expiry_date(), context);
    double nearest = grid.times.back();
    if (!observation_events.empty()) nearest = std::min(nearest, observation_events.front().time);
    if (!trading_times.empty()) nearest = std::min(nearest, trading_times.front());
    const double days = std::min(1.0, nearest * 365.0);
    const double scale = settings.scheme == FiniteDifferenceScheme::explicit_euler ? 0.0
                                                                                   : workspace.contract_level * std::max(0.001 * std::sqrt(days), 64.0 * std::numeric_limits<double>::epsilon());
    // The request fixes spot, valuation and contract levels; only these geometry inputs can change.
    if (!workspace.space || workspace.requested_upper != upper || workspace.concentration_scale != scale) {
        auto space = make_spatial_grid(settings, upper, {relevant}, upper > 4.0 * workspace.contract_level ? workspace.contract_level : 0.0);
        if (!space) return std::unexpected(space.error());
        if (settings.scheme != FiniteDifferenceScheme::explicit_euler) {
            std::vector<double> levels{program.initial_spot, program.knock_in_level, program.lower_strike, program.upper_strike};
            for (const auto& observation : workspace.observation_events) {
                levels.push_back(observation.event.knock_out_level);
                if (observation.event.conditional_coupon) levels.push_back(observation.event.coupon_barrier);
            }
            // Follow diffusion width near events without introducing nearly coincident nodes.
            *space = concentrate_spatial_grid(std::move(*space), std::move(levels), scale);
        }
        if (!workspace.space || workspace.space->upper != space->upper || workspace.space->sinh_scale != space->sinh_scale || workspace.space->nodes != space->nodes) {
            workspace.stepper.reset();
            workspace.assets.resize(space->size());
            for (std::size_t index = 0; index < workspace.assets.size(); ++index)
                workspace.assets[index] = space->asset_at(index);
        }
        workspace.space = std::move(*space);
        workspace.requested_upper = upper;
        workspace.concentration_scale = scale;
    }
    return PreparedAutocallable{program, initial, std::nullopt};
}

template <AutocallableProduct Note>
Result<PricingResult> price_autocallable(
    const Note& note, const PricingContext& context, const FiniteDifferenceSettings& settings,
    AutocallableFiniteDifferenceRequest<Note>& request, GreeksRequest output, double spot_shift)
{
    const auto prepared = prepare_autocallable(note, context, settings, request, output);
    if (!prepared) return std::unexpected(prepared.error());
    if (prepared->settled) return *prepared->settled;
    return rollback_autocallable(prepared->program, prepared->initial, context, settings, output, spot_shift, request.workspace);
}

template <AutocallableProduct Note>
Result<std::array<double, 2>> price_coupon_endpoints(
    const Note& lower, const Note& upper, const PricingContext& context,
    const FiniteDifferenceSettings& settings, AutocallableFiniteDifferenceRequest<Note>& request)
{
    AutocallableFiniteDifferenceRequest<Note> alternate{{upper, context.calendar()}, {}};
    const auto first = prepare_autocallable(lower, context, settings, request, {});
    if (!first) return std::unexpected(first.error());
    const auto second = prepare_autocallable(upper, context, settings, alternate, {});
    if (!second) return std::unexpected(second.error());
    std::array states{AutocallableRollback{first->program, first->initial, context, request.workspace},
                      AutocallableRollback{second->program, second->initial, context, alternate.workspace}};
    const auto& main = request.workspace;
    const auto& other = alternate.workspace;
    const bool compatible = !first->settled && !second->settled && states[0].layered() == states[1].layered() && main.space && other.space &&
                            main.space->upper == other.space->upper && main.space->spacing == other.space->spacing &&
                            main.space->sinh_scale == other.space->sinh_scale && main.space->nodes == other.space->nodes &&
                            main.grid.times == other.grid.times && main.grid.time_steps == other.grid.time_steps;
    if (compatible) {
        const auto results = rollback_autocallables(states, context, settings, {}, NumericalShiftSettings{}.spot_shift);
        if (!results) return std::unexpected(results.error());
        return std::array{(*results)[0].price(), (*results)[1].price()};
    }
    // A coupon can remove knock-in exposure and change the required grid or fixed settlement.
    const auto value = [&](const PreparedAutocallable& prepared, auto& storage) -> Result<double> {
        if (prepared.settled) return prepared.settled->price();
        return price_value(rollback_autocallable(prepared.program, prepared.initial, context, settings, {},
                                                 NumericalShiftSettings{}.spot_shift, storage.workspace));
    };
    const auto low = value(*first, request);
    if (!low) return std::unexpected(low.error());
    const auto high = value(*second, alternate);
    if (!high) return std::unexpected(high.error());
    return std::array{*low, *high};
}

template <typename Note, typename ReplaceCoupon>
Result<double> solve_coupon(
    const FiniteDifferenceAutocallableEngine& engine, const Note& note, const PricingContext& context,
    double observed_price, ImpliedCouponSettings settings, const ReplaceCoupon& replace)
{
    AutocallableFiniteDifferenceRequest<Note> request{{note, context.calendar()}, {}};
    return detail::solve_implied_coupon(engine, note, context, observed_price, settings, replace, [&](const auto& scalar, const Note& instrument, const PricingContext& market) { return detail::price_value(price_autocallable(
                                                                                                                                                                                      instrument, market, scalar.settings(), request, GreeksRequest{},
                                                                                                                                                                                      NumericalShiftSettings{}.spot_shift)); }, [&](const auto& scalar, const Note& lower, const Note& upper, const PricingContext& market) { return price_coupon_endpoints(lower, upper, market, scalar.settings(), request); });
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
