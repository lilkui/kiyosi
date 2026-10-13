#include <kiyosi/pricing/engines/autocallable/finite_difference.hpp>

#include <kiyosi/pricing/numerical_greeks.hpp>
#include <kiyosi/pricing/implied.hpp>

#include <algorithm>
#include <cmath>
#include <optional>
#include <vector>

#include <kiyosi/market/schedule.hpp>
#include <kiyosi/pricing/detail/autocallable_traits.hpp>

#include "../../detail/calendar_dates.hpp"
#include "../../detail/autocallable_pricer.hpp"
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

// Reuse only across coupon or volatility changes; all other inputs must stay fixed.
struct AutocallableFiniteDifferenceWorkspace {
    std::optional<SpatialGrid> space;
    std::optional<LinearBoundaryStepper> stepper;
    std::optional<bool> daily_grid;
    FiniteDifferenceTimeGrid grid;
    std::vector<ObservationEvent> observation_events;
    std::vector<double> trading_times;
    std::vector<double> alive, next_alive, knocked_in, next_knocked_in, curvature;
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
    const auto& space = workspace.space;
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
    if (auto stable = check_explicit_stability(settings.scheme, grid, sigma, rate, dividend, settings.asset_step_count);
        !stable)
        return std::unexpected(stable.error());

    const auto event_at = [&](double time) {
        const auto found = std::lower_bound(
            observation_events.begin(), observation_events.end(), time,
            [](const ObservationEvent& event, double value) { return event.time < value; });
        return found == observation_events.end() || found->time != time
                   ? AutocallableEvent{}
                   : found->event;
    };

    const std::size_t size = space->size();
    const auto asset = [&](std::size_t index) { return space->spacing * static_cast<double>(index); };
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
    const auto expiry_event = event_at(time_to_expiry);
    for (std::size_t index = 0; index < size; ++index) {
        const double value = asset(index);
        const bool ki = initial.path.knocked_in || (program.has_knock_in && value < program.knock_in_level);
        if (expiry_event.active && value >= expiry_event.knock_out_level) {
            alive[index] = program.principal_ratio +
                           program_observation_coupon(expiry_event, value);
            if (needs_knock_in_layer) knocked_in[index] = alive[index];
        } else {
            const double coupon = expiry_event.active && program.carries_observation_coupon
                                      ? program_observation_coupon(expiry_event, value)
                                      : 0.0;
            alive[index] = program_terminal_settlement(program, value, ki) + coupon;
            if (needs_knock_in_layer)
                knocked_in[index] = program_terminal_settlement(program, value, true) + coupon;
        }
    }

    if (!workspace.stepper)
        workspace.stepper.emplace(size, space->upper, space->spacing,
                                  DiffusionParameters{rate, dividend, sigma, scheme_theta(settings.scheme)});
    auto& stepper = *workspace.stepper;
    stepper.set_parameters({rate, dividend, sigma, scheme_theta(settings.scheme)});
    for (std::size_t step = grid.time_steps.size(); step-- > 0;) {
        const double dt = grid.time_steps[step];
        const bool advanced = [&] {
            if (needs_knock_in_layer)
                return stepper.advance_pair(knocked_in, next_knocked_in, alive, next_alive, dt);
            else
                return stepper.advance(alive, next_alive, dt);
        }();
        if (!advanced)
            return std::unexpected(Error{ErrorCategory::invalid_result,
                                         "finite-difference system is numerically unstable"});
        const auto event = event_at(grid.times[step]);
        const bool daily = monitors_daily &&
                           std::ranges::binary_search(trading_times, grid.times[step]);
        if (event.active || (needs_knock_in_layer && daily)) {
            for (std::size_t index = 0; index < size; ++index) {
                const double value = asset(index);
                const bool transitioned = needs_knock_in_layer && daily && value < program.knock_in_level;
                if (event.active && value >= event.knock_out_level) {
                    next_alive[index] = program.principal_ratio +
                                        program_observation_coupon(event, value);
                    if (needs_knock_in_layer) next_knocked_in[index] = next_alive[index];
                } else if (event.active) {
                    const double coupon = program.carries_observation_coupon
                                              ? program_observation_coupon(event, value)
                                              : 0.0;
                    if (needs_knock_in_layer) {
                        const double continuation_in = next_knocked_in[index];
                        const double continuation_out = transitioned ? continuation_in : next_alive[index];
                        next_knocked_in[index] = continuation_in + coupon;
                        next_alive[index] = continuation_out + coupon;
                    } else {
                        next_alive[index] = next_alive[index] + coupon;
                    }
                } else if (transitioned) {
                    next_alive[index] = next_knocked_in[index];
                }
            }
        }
        alive.swap(next_alive);
        if (needs_knock_in_layer) knocked_in.swap(next_knocked_in);
    }

    auto& curvature = workspace.curvature;
    curvature.clear();
    const double value = initial.path.coupons + space->interpolate(alive, spot, curvature);
    return finite_difference_result(value, spot, space->upper, output, spot_shift, [&](double shifted_spot) { return checked_price(initial.path.coupons + space->interpolate(alive, shifted_spot, curvature)); }, [&](double shifted_spot) { return space->delta(alive, shifted_spot); }, [&](double shifted_spot) { return space->gamma(alive, shifted_spot); });
}

template <AutocallableProduct Note>
Result<PricingResult> price_autocallable(
    const Note& note, const PricingContext& context, const FiniteDifferenceSettings& settings,
    AutocallableFiniteDifferenceRequest<Note>& request, GreeksRequest output, double spot_shift)
{
    const auto valid = request.contract.validate(context);
    if (!valid) return std::unexpected(valid.error());
    auto settings_valid = detail::validate_finite_difference_settings(
        settings, trading_fd_max_steps, trading_fd_max_steps);
    if (!settings_valid) return std::unexpected(settings_valid.error());

    auto& workspace = request.workspace;
    const auto program = autocallable_program(note);
    const auto initial = autocallable_initial_state(note, context, program);

    if (initial.settlement) return make_pricing_result(*initial.settlement);
    if (const auto fixed = autocallable_fixed_value(note, context, program, initial))
        return make_pricing_result(*fixed,
                                   {{Greek::delta, output.has(Greek::delta) ? std::optional{0.0} : std::nullopt},
                                    {Greek::gamma, output.has(Greek::gamma) ? std::optional{0.0} : std::nullopt}});

    if (!workspace.space) {
        const double relevant = highest_finite_difference_level(note, context);
        const auto space = make_spatial_grid(settings, default_finite_difference_upper_boundary(note, context), {relevant});
        if (!space) return std::unexpected(space.error());
        workspace.space = *space;
    }

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
        observation_events.reserve(note.observation_dates().size());
        for (std::size_t index = 0; index < note.observation_dates().size(); ++index) {
            const Date value = note.observation_dates()[index];
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
        workspace.daily_grid = daily_grid;
    }

    for (auto& observation : workspace.observation_events)
        observation.event = autocallable_event(note, observation.index);
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
    return detail::price_with_greeks(*this, option, context, greeks, settings,
                                     [&](const auto& engine, const PricingContext& shifted, GreeksRequest requested_output) {
                                         // Greek shifts may change time or rates; only contract validation survives.
                                         request.workspace = {};
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
