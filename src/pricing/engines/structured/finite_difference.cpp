#include <kiyosi/pricing/engines/structured/finite_difference.hpp>

#include <algorithm>
#include <cmath>
#include <optional>
#include <vector>

#include <kiyosi/market/schedule.hpp>

#include "../../detail/autocallable_traits.hpp"
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

/// Highest product level the grid must span so no barrier or strike is clipped.
template <typename Note>
double highest_relevant_level(const Note& note, double spot)
{
    double relevant = std::max({spot, note.initial_spot(), note.upper_strike(), note.lower_strike()});
    for (const double level : note.knock_out_levels())
        relevant = std::max(relevant, level);
    if constexpr (requires { note.knock_in_level(); })
        relevant = std::max(relevant, note.knock_in_level());
    if constexpr (requires { note.coupon_barrier_levels(); })
        for (const double level : note.coupon_barrier_levels())
            relevant = std::max(relevant, level);
    return relevant;
}

template <typename Note>
Result<PricingResult> terminal_value(const Note& note, const PricingContext& context)
{
    const double spot = context.spot_price();
    const bool knocked_in =
        is_knocked_in(note, spot, note.barrier_state() == AutocallableBarrierState::knocked_in, true);
    const auto& dates = note.observation_dates();
    std::size_t index = 0;
    while (index < dates.size() && dates[index] < note.expiry_date())
        ++index;
    const bool observed_at_expiry = index < dates.size() && dates[index] == note.expiry_date();
    if (observed_at_expiry && spot >= note.knock_out_levels()[index])
        return make_pricing_result(
            {{RiskMeasure::price,
              note.principal_ratio() + observation_coupon(note, index, spot)}});
    const double coupon = observed_at_expiry && carries_observation_coupon<Note>
                              ? observation_coupon(note, index, spot)
                              : 0.0;
    return make_pricing_result(
        {{RiskMeasure::price, terminal_settlement(note, spot, knocked_in) + coupon}});
}

} // namespace

template <typename Note>
Result<PricingResult> price_autocallable_finite_difference(
    const Note& note, const PricingContext& context, FiniteDifferenceSettings settings)
{
    auto valid = validate_valuation_within_instrument_life(context.valuation_time(), note.effective_date(), note.expiry_date());
    if (!valid) return std::unexpected(valid.error());
    auto expiry_valid = validate_trading_expiry(context.calendar(), note.expiry_date());
    if (!expiry_valid) return std::unexpected(expiry_valid.error());
    auto settings_valid = validate_finite_difference_settings(settings);
    if (!settings_valid) return std::unexpected(settings_valid.error());
    if (settings.asset_step_count > 2000 || settings.time_step_count > 2000)
        return std::unexpected(Error{ErrorCategory::invalid_parameter,
                                     "finite-difference grid dimensions are out of range"});
    auto note_validation = validate_autocallable_note(note);
    if (!note_validation) return std::unexpected(note_validation.error());
    auto schedule = validate_observation_dates(note.observation_dates(), note.effective_date(),
                                               note.expiry_date(), context.calendar());
    if (!schedule) return std::unexpected(schedule.error());

    // An up-touch has already autocalled the note, so nothing remains to discount.
    if (note.barrier_state() == AutocallableBarrierState::knocked_out && !settings.asset_upper_boundary)
        return make_pricing_result({{RiskMeasure::price, 0.0}});

    const double spot = context.spot_price();
    const double relevant = highest_relevant_level(note, spot);
    const auto space = make_spatial_grid(settings, std::max(4.0 * relevant, relevant + 1.0), {relevant});
    if (!space) return std::unexpected(space.error());
    if (note.barrier_state() == AutocallableBarrierState::knocked_out)
        return make_pricing_result({{RiskMeasure::price, 0.0}});

    const Timestamp valuation = context.valuation_time();
    const double time_to_expiry = actual_365_fixed_year_fraction(valuation, note.expiry_date());
    if (time_to_expiry == 0.0) return terminal_value(note, context);

    std::vector<double> anchors{0.0, time_to_expiry};
    std::vector<ObservationEvent> observation_events;
    observation_events.reserve(note.observation_dates().size());
    for (std::size_t index = 0; index < note.observation_dates().size(); ++index) {
        const Date value = note.observation_dates()[index];
        if (value < valuation) continue;
        const double time = actual_365_fixed_year_fraction(valuation, value);
        observation_events.push_back({time, index});
        if (time > 0.0 && time < time_to_expiry) anchors.push_back(time);
    }
    constexpr bool monitors_knock_in = requires(const Note& value) { value.knock_in_observation_mode(); };
    bool monitors_daily = false;
    if constexpr (monitors_knock_in)
        monitors_daily = note.knock_in_observation_mode() == KnockInObservationMode::every_trading_day;
    std::vector<double> trading_times;
    if (monitors_daily) {
        const auto future_trading_dates =
            trading_dates(context.calendar(), valuation, note.expiry_date(), true);
        trading_times.reserve(future_trading_dates.size());
        for (const Date value : future_trading_dates) {
            const double time = actual_365_fixed_year_fraction(valuation, value);
            trading_times.push_back(time);
            if (time > 0.0 && time < time_to_expiry) anchors.push_back(time);
        }
    }

    const double rate = context.model_parameters().risk_free_rate();
    const double dividend = context.model_parameters().dividend_yield();
    const double sigma = context.model_parameters().volatility();
    const auto grid = make_finite_difference_time_grid(time_to_expiry, settings.time_step_count, std::move(anchors));
    if (auto stable = check_explicit_stability(settings.scheme, grid, sigma, rate, settings.asset_step_count);
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
    std::vector<double> alive(size), next_alive(size);
    std::vector<double> knocked_in, next_knocked_in;
    if constexpr (monitors_knock_in) {
        knocked_in.resize(size);
        next_knocked_in.resize(size);
    }
    const auto expiry_observation = event_index(time_to_expiry);
    for (std::size_t index = 0; index < size; ++index) {
        const double value = asset(index);
        bool ki = note.barrier_state() == AutocallableBarrierState::knocked_in;
        if constexpr (monitors_knock_in) ki = ki || value < note.knock_in_level();
        if (expiry_observation && value >= note.knock_out_levels()[*expiry_observation]) {
            alive[index] = note.principal_ratio() +
                           observation_coupon(note, *expiry_observation, value);
            if constexpr (monitors_knock_in) knocked_in[index] = alive[index];
        } else {
            const double coupon = expiry_observation && carries_observation_coupon<Note>
                                      ? observation_coupon(note, *expiry_observation, value)
                                      : 0.0;
            alive[index] = terminal_settlement(note, value, ki) + coupon;
            if constexpr (monitors_knock_in)
                knocked_in[index] = terminal_settlement(note, value, true) + coupon;
        }
    }

    LinearBoundaryStepper stepper(
        size, space->upper, space->spacing,
        DiffusionParameters{rate, dividend, sigma, scheme_theta(settings.scheme)});
    for (std::size_t step = grid.size() - 1; step-- > 0;) {
        const double dt = grid[step + 1] - grid[step];
        bool advanced;
        if constexpr (monitors_knock_in)
            advanced = stepper.advance_pair(knocked_in, next_knocked_in, alive, next_alive, dt);
        else
            advanced = stepper.advance(alive, next_alive, dt);
        if (!advanced)
            return std::unexpected(Error{ErrorCategory::invalid_result,
                                         "finite-difference system is numerically unstable"});
        const auto observation_index = event_index(grid[step]);
        const bool daily = monitors_daily &&
                           std::binary_search(trading_times.begin(), trading_times.end(), grid[step]);
        for (std::size_t index = 0; index < size; ++index) {
            const double value = asset(index);
            bool transitioned = false;
            if constexpr (monitors_knock_in) transitioned = daily && value < note.knock_in_level();
            if (observation_index && value >= note.knock_out_levels()[*observation_index]) {
                next_alive[index] = note.principal_ratio() +
                                    observation_coupon(note, *observation_index, value);
                if constexpr (monitors_knock_in) next_knocked_in[index] = next_alive[index];
            } else if (observation_index) {
                const double coupon = carries_observation_coupon<Note>
                                          ? observation_coupon(note, *observation_index, value)
                                          : 0.0;
                if constexpr (monitors_knock_in) {
                    const double continuation_in = next_knocked_in[index];
                    const double continuation_out = transitioned ? continuation_in : next_alive[index];
                    next_knocked_in[index] = continuation_in + coupon;
                    next_alive[index] = continuation_out + coupon;
                } else {
                    next_alive[index] = next_alive[index] + coupon;
                }
            } else if (transitioned) {
                if constexpr (monitors_knock_in) next_alive[index] = next_knocked_in[index];
            }
        }
        alive.swap(next_alive);
        if constexpr (monitors_knock_in) knocked_in.swap(next_knocked_in);
    }

    if constexpr (monitors_knock_in)
        return make_pricing_result(
            {{RiskMeasure::price,
              space->interpolate(note.barrier_state() == AutocallableBarrierState::knocked_in ? knocked_in
                                                                                              : alive,
                                 spot)}});
    else
        return make_pricing_result({{RiskMeasure::price, space->interpolate(alive, spot)}});
}

template Result<PricingResult> price_autocallable_finite_difference(
    const PhoenixOption&, const PricingContext&, FiniteDifferenceSettings);
template Result<PricingResult> price_autocallable_finite_difference(
    const SnowballOption&, const PricingContext&, FiniteDifferenceSettings);
template Result<PricingResult> price_autocallable_finite_difference(
    const BinarySnowballOption&, const PricingContext&, FiniteDifferenceSettings);
template Result<PricingResult> price_autocallable_finite_difference(
    const TernarySnowballOption&, const PricingContext&, FiniteDifferenceSettings);

} // namespace kiyosi
