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
#include "../../detail/fd_grid.hpp"
#include "../../detail/fd_scheme.hpp"
#include "../../detail/math.hpp"

namespace kiyosi {
using namespace detail;

namespace {

struct ObservationEvent {
    double time;
    std::size_t index;
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
Result<PricingResult> price_autocallable(
    const Note& note, const PricingContext& context, const FiniteDifferenceSettings& settings,
    bool& contract_validated, GreeksRequest output, double spot_shift,
    AutocallableFiniteDifferenceWorkspace& workspace)
{
    auto valid = validate_valuation_within_instrument_life(context.valuation_time(), note.effective_date(), note.expiry_date());
    if (!valid) return std::unexpected(valid.error());
    auto expiry_valid = validate_trading_expiry(context.calendar(), note.expiry_date());
    if (!expiry_valid) return std::unexpected(expiry_valid.error());
    auto settings_valid = detail::validate_finite_difference_settings(
        settings, trading_fd_max_steps, trading_fd_max_steps);
    if (!settings_valid) return std::unexpected(settings_valid.error());
    if (!contract_validated) {
        const auto note_validation = validate_autocallable_note(note);
        if (!note_validation) return std::unexpected(note_validation.error());
        const auto schedule = validate_observation_trading_days(note.observation_dates(), context.calendar());
        if (!schedule) return std::unexpected(schedule.error());
        contract_validated = true;
    }
    auto history = validate_autocallable_history(note, context);
    if (!history) return std::unexpected(history.error());

    const auto program = autocallable_program(note);
    const auto initial = autocallable_initial_state(note, context, program);

    if (initial.settlement) return make_pricing_result(*initial.settlement);
    if (const auto fixed = autocallable_fixed_value(note, context, program, initial))
        return make_pricing_result(*fixed,
                                   {{Greek::delta, output.has(Greek::delta) ? std::optional{0.0} : std::nullopt},
                                    {Greek::gamma, output.has(Greek::gamma) ? std::optional{0.0} : std::nullopt}});

    const double spot = context.spot_price();
    if (!workspace.space) {
        const double relevant = highest_finite_difference_level(note, context);
        const auto space = make_spatial_grid(settings, default_finite_difference_upper_boundary(note, context), {relevant});
        if (!space) return std::unexpected(space.error());
        workspace.space = *space;
    }
    const auto& space = workspace.space;

    const Timestamp valuation = context.valuation_time();
    const double time_to_expiry = actual_365_fixed_year_fraction(valuation, note.expiry_date());

    constexpr bool monitors_knock_in = requires(const Note& value) { value.knock_in_observation_mode(); };
    bool monitors_daily = false; // NOLINT(misc-const-correctness): assigned for knock-in note types.
    if constexpr (monitors_knock_in)
        monitors_daily = note.knock_in_observation_mode() == KnockInObservationMode::every_trading_day;
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

    const double rate = context.model_parameters().risk_free_rate();
    const double dividend = context.model_parameters().dividend_yield();
    const double sigma = context.model_parameters().volatility();
    if (auto stable = check_explicit_stability(settings.scheme, grid, sigma, rate, dividend, settings.asset_step_count);
        !stable)
        return std::unexpected(stable.error());

    const auto event_index = [&](double time) -> std::optional<std::size_t> {
        const auto found = std::lower_bound(
            observation_events.begin(), observation_events.end(), time,
            [](const ObservationEvent& event, double value) { return event.time < value; });
        return found == observation_events.end() || found->time != time
                   ? std::nullopt
                   : std::optional<std::size_t>{found->index};
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
    const auto expiry_observation = event_index(time_to_expiry);
    const auto expiry_event = expiry_observation ? autocallable_event(note, *expiry_observation) : AutocallableEvent{};
    for (std::size_t index = 0; index < size; ++index) {
        const double value = asset(index);
        bool ki = initial.path.knocked_in; // NOLINT(misc-const-correctness): updated for knock-in note types.
        if constexpr (monitors_knock_in) ki = ki || value < note.knock_in_level();
        if (expiry_event.active && value >= expiry_event.knock_out_level) {
            alive[index] = note.principal_ratio() +
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
        const auto observation_index = event_index(grid.times[step]);
        const auto event = observation_index ? autocallable_event(note, *observation_index) : AutocallableEvent{};
        const bool daily = monitors_daily &&
                           std::ranges::binary_search(trading_times, grid.times[step]);
        if (event.active || (needs_knock_in_layer && daily)) {
            for (std::size_t index = 0; index < size; ++index) {
                const double value = asset(index);
                bool transitioned = false; // NOLINT(misc-const-correctness): updated for knock-in note types.
                if constexpr (monitors_knock_in) transitioned = needs_knock_in_layer && daily && value < note.knock_in_level();
                if (event.active && value >= event.knock_out_level) {
                    next_alive[index] = note.principal_ratio() +
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
    const double h = output.has(Greek::speed) ? symmetric_shift(spot, spot_shift) : 0.0;
    const auto speed = finite_difference_speed(spot, h, space->upper, [&](double shifted_spot) { return checked_price(initial.path.coupons + space->interpolate(alive, shifted_spot, curvature)); }, [&](double shifted_spot) { return space->gamma(alive, shifted_spot); });
    if (!speed) return std::unexpected(speed.error());
    return make_pricing_result(value,
                               {{Greek::delta, output.has(Greek::delta) ? std::optional{space->delta(alive, spot)} : std::nullopt},
                                {Greek::gamma, output.has(Greek::gamma) ? std::optional{space->gamma(alive, spot)} : std::nullopt},
                                {Greek::speed, *speed}});
}

template <typename Note, typename ReplaceCoupon>
Result<double> solve_coupon(
    const FiniteDifferenceAutocallableEngine<Note>& engine, const Note& note, const PricingContext& context,
    double observed_price, ImpliedCouponSettings settings, const ReplaceCoupon& replace)
{
    AutocallableFiniteDifferenceWorkspace workspace;
    bool contract_validated = false;
    return detail::solve_implied_coupon(engine, note, context, observed_price, settings, replace,
                                        [&](const auto& scalar, const Note& instrument, const PricingContext& market) {
                                            return detail::price_value(price_autocallable(
                                                instrument, market, scalar.settings(), contract_validated, GreeksRequest{},
                                                NumericalShiftSettings{}.spot_shift, workspace));
                                        });
}

} // namespace

template <AutocallableProduct Note>
Result<PricingResult> FiniteDifferenceAutocallableEngine<Note>::price_native(
    const Note& note, const PricingContext& context, bool& contract_validated,
    GreeksRequest output, double spot_shift) const
{
    AutocallableFiniteDifferenceWorkspace workspace;
    return price_autocallable(note, context, settings_, contract_validated, output, spot_shift, workspace);
}

template <AutocallableProduct Note>
Result<double> implied_volatility(
    const FiniteDifferenceAutocallableEngine<Note>& engine, const Note& option, const PricingContext& context,
    double observed_price, ImpliedVolatilitySettings settings)
{
    AutocallableFiniteDifferenceWorkspace workspace;
    bool contract_validated = false;
    return detail::solve_implied_volatility(engine, option, context, observed_price, settings,
                                            [&](const auto& scalar, const Note& instrument, const PricingContext& market) {
                                                return detail::price_value(price_autocallable(
                                                    instrument, market, scalar.settings(), contract_validated, GreeksRequest{},
                                                    NumericalShiftSettings{}.spot_shift, workspace));
                                            });
}

template <AutocallableProduct Note>
Result<double> implied_coupon(
    const FiniteDifferenceAutocallableEngine<Note>& engine, const Note& option, const PricingContext& context,
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
    const FiniteDifferencePhoenixEngine& engine, const PhoenixOption& option, const PricingContext& context,
    double observed_price, ImpliedCouponSettings settings)
{
    return solve_coupon(engine, option, context, observed_price, settings, [](const auto& note, double coupon) {
        return detail::replace_coupon(note, coupon);
    });
}

template <AutocallableProduct Note>
Result<double> FiniteDifferenceAutocallableEngine<Note>::price(
    const Note& note, const PricingContext& context) const
{
    bool contract_validated = false;
    return detail::price_value(price_native(note, context, contract_validated, GreeksRequest{}));
}

template <AutocallableProduct Note>
Result<PricingResult> FiniteDifferenceAutocallableEngine<Note>::price_with_greeks(const Note& option, const PricingContext& context,
                                                                                  GreeksRequest greeks, NumericalShiftSettings settings) const
{
    bool contract_validated = false;
    return detail::price_with_greeks(*this, option, context, greeks, settings,
                                     [&](const auto& engine, const PricingContext& shifted, GreeksRequest request) {
                                         const auto output = detail::at_spot_discontinuity(option, shifted, detail::symmetric_shift(shifted.spot_price(), settings.spot_shift)) ? GreeksRequest{} : request;
                                         return engine.price_native(option, shifted, contract_validated, output, settings.spot_shift);
                                     });
}

template KIYOSI_EXPORT Result<double> implied_volatility(
    const FiniteDifferenceAutocallableEngine<PhoenixOption>&, const PhoenixOption&,
    const PricingContext&, double, ImpliedVolatilitySettings);
template KIYOSI_EXPORT Result<double> implied_volatility(
    const FiniteDifferenceAutocallableEngine<SnowballOption>&, const SnowballOption&,
    const PricingContext&, double, ImpliedVolatilitySettings);
template KIYOSI_EXPORT Result<double> implied_volatility(
    const FiniteDifferenceAutocallableEngine<BinarySnowballOption>&, const BinarySnowballOption&,
    const PricingContext&, double, ImpliedVolatilitySettings);
template KIYOSI_EXPORT Result<double> implied_volatility(
    const FiniteDifferenceAutocallableEngine<TernarySnowballOption>&, const TernarySnowballOption&,
    const PricingContext&, double, ImpliedVolatilitySettings);
template KIYOSI_EXPORT Result<double> implied_coupon(
    const FiniteDifferenceAutocallableEngine<SnowballOption>&, const SnowballOption&,
    const PricingContext&, double, CouponQuoteConvention, ImpliedCouponSettings);
template KIYOSI_EXPORT Result<double> implied_coupon(
    const FiniteDifferenceAutocallableEngine<BinarySnowballOption>&, const BinarySnowballOption&,
    const PricingContext&, double, CouponQuoteConvention, ImpliedCouponSettings);
template KIYOSI_EXPORT Result<double> implied_coupon(
    const FiniteDifferenceAutocallableEngine<TernarySnowballOption>&, const TernarySnowballOption&,
    const PricingContext&, double, CouponQuoteConvention, ImpliedCouponSettings);

template class FiniteDifferenceAutocallableEngine<PhoenixOption>;
template class FiniteDifferenceAutocallableEngine<SnowballOption>;
template class FiniteDifferenceAutocallableEngine<BinarySnowballOption>;
template class FiniteDifferenceAutocallableEngine<TernarySnowballOption>;

} // namespace kiyosi
