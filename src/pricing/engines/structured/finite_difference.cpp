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

/// Highest product level the grid must span so no barrier or strike is clipped.
template <typename Note>
double highest_relevant_level(const Note& note, double spot)
{
    double relevant = std::max({spot, note.initial_price(), note.upper_strike(), note.lower_strike()});
    for (const double level : note.knock_out_prices()) relevant = std::max(relevant, level);
    if constexpr (requires { note.knock_in_price(); })
        relevant = std::max(relevant, note.knock_in_price());
    if constexpr (requires { note.coupon_barriers(); })
        for (const double level : note.coupon_barriers()) relevant = std::max(relevant, level);
    return relevant;
}

template <typename Note>
result<PricingResult> terminal_value(const Note& note, const PricingContext& context)
{
    const double spot = context.asset_price();
    const bool knocked_in =
        is_knocked_in(note, spot, note.touch_status() == barrier_touch_status::down, true);
    const auto& dates = note.observation_dates();
    std::size_t index = 0;
    while (index < dates.size() && dates[index] < note.expiry()) ++index;
    const bool observed_at_expiry = index < dates.size() && dates[index] == note.expiry();
    if (observed_at_expiry && spot >= note.knock_out_prices()[index])
        return PricingResult{{risk_measure::price,
                              note.principal_ratio() + observation_coupon(note, index, spot)}};
    const double coupon = observed_at_expiry && carries_observation_coupon<Note>
                              ? observation_coupon(note, index, spot)
                              : 0.0;
    return PricingResult{{risk_measure::price, terminal_settlement(note, spot, knocked_in) + coupon}};
}

} // namespace

template <typename Note>
result<PricingResult> price_autocallable_finite_difference(
    const Note& note, const PricingContext& context, FiniteDifferenceSettings settings)
{
    auto valid = validate_life(context.valuation_time(), note.effective(), note.expiry());
    if (!valid) return std::unexpected(valid.error());
    auto settings_valid = validate_finite_difference_settings(settings);
    if (!settings_valid) return std::unexpected(settings_valid.error());
    if (settings.asset_steps > 2000 || settings.time_steps > 2000)
        return std::unexpected(Error{error_category::invalid_parameter,
                                     "finite-difference grid dimensions are out of range"});
    auto contract = validate_note(note);
    if (!contract) return std::unexpected(contract.error());
    auto schedule = validate_observation_dates(note.observation_dates(), note.effective(),
                                               note.expiry(), context.calendar());
    if (!schedule) return std::unexpected(schedule.error());

    // An up-touch has already autocalled the note, so nothing remains to discount.
    if (note.touch_status() == barrier_touch_status::up && !settings.upper_boundary)
        return PricingResult{{risk_measure::price, 0.0}};

    const double spot = context.asset_price();
    const double relevant = highest_relevant_level(note, spot);
    const auto space = make_spatial_grid(settings, std::max(4.0 * relevant, relevant + 1.0), {relevant});
    if (!space) return std::unexpected(space.error());
    if (note.touch_status() == barrier_touch_status::up)
        return PricingResult{{risk_measure::price, 0.0}};

    const double maturity = actual_365(context.valuation_time(), note.expiry());
    if (maturity == 0.0) return terminal_value(note, context);

    const auto future_trading_dates =
        trading_dates(context.calendar(), context.valuation_time(), note.expiry(), true);
    std::vector<double> anchors{0.0, maturity};
    const auto add_anchor = [&](date value) {
        if (value <= context.valuation_time()) return;
        const double time = actual_365(context.valuation_time(), value);
        if (time > 0.0 && time < maturity) anchors.push_back(time);
    };
    for (const date value : note.observation_dates()) add_anchor(value);
    constexpr bool monitors_knock_in = requires(const Note& value) { value.knock_in_frequency(); };
    bool monitors_daily = false;
    if constexpr (monitors_knock_in)
        monitors_daily = note.knock_in_frequency() == observation_frequency::daily;
    if (monitors_daily)
        for (const date value : future_trading_dates) add_anchor(value);

    const double rate = context.parameters().risk_free_rate();
    const double dividend = context.parameters().dividend_yield();
    const double sigma = context.parameters().volatility();
    const auto grid = finite_difference_grid(maturity, settings.time_steps, std::move(anchors));
    if (auto stable = check_explicit_stability(settings.scheme, grid, sigma, rate, settings.asset_steps);
        !stable)
        return std::unexpected(stable.error());

    const auto observation_events = observation_schedule(note, context.valuation_time());
    const auto event_index = [&](double time) -> std::optional<std::size_t> {
        const auto found = std::find_if(
            observation_events.begin(), observation_events.end(), [&](std::size_t index) {
                return actual_365(context.valuation_time(), note.observation_dates()[index]) == time;
            });
        return found == observation_events.end() ? std::nullopt : std::optional<std::size_t>{*found};
    };
    const auto daily_event = [&](double time) {
        return monitors_daily &&
               std::any_of(future_trading_dates.begin(), future_trading_dates.end(), [&](date value) {
                   return actual_365(context.valuation_time(), value) == time;
               });
    };

    const std::size_t size = space->size();
    const auto asset = [&](std::size_t index) { return space->spacing * static_cast<double>(index); };
    std::vector<double> knocked_in(size), alive(size), next_knocked_in(size), next_alive(size);
    const auto expiry_observation = event_index(maturity);
    for (std::size_t index = 0; index < size; ++index) {
        const double value = asset(index);
        bool ki = note.touch_status() == barrier_touch_status::down;
        if constexpr (monitors_knock_in) ki = ki || value < note.knock_in_price();
        if (expiry_observation && value >= note.knock_out_prices()[*expiry_observation]) {
            knocked_in[index] = alive[index] =
                note.principal_ratio() + observation_coupon(note, *expiry_observation, value);
        } else {
            const double coupon = expiry_observation && carries_observation_coupon<Note>
                                      ? observation_coupon(note, *expiry_observation, value)
                                      : 0.0;
            knocked_in[index] = terminal_settlement(note, value, true) + coupon;
            alive[index] = terminal_settlement(note, value, ki) + coupon;
        }
    }

    LinearBoundaryStepper stepper(
        size, space->upper, space->spacing,
        DiffusionParameters{rate, dividend, sigma, scheme_theta(settings.scheme)});
    for (std::size_t step = grid.size() - 1; step-- > 0;) {
        const double dt = grid[step + 1] - grid[step];
        if (!stepper.advance(knocked_in, next_knocked_in, dt) ||
            !stepper.advance(alive, next_alive, dt))
            return std::unexpected(Error{error_category::invalid_result,
                                         "finite-difference system is numerically unstable"});
        const auto observation = event_index(grid[step]);
        const bool daily = daily_event(grid[step]);
        for (std::size_t index = 0; index < size; ++index) {
            const double value = asset(index);
            bool transitioned = false;
            if constexpr (monitors_knock_in) transitioned = daily && value < note.knock_in_price();
            if (observation && value >= note.knock_out_prices()[*observation]) {
                next_knocked_in[index] = next_alive[index] =
                    note.principal_ratio() + observation_coupon(note, *observation, value);
            } else if (observation) {
                const double coupon = carries_observation_coupon<Note>
                                          ? observation_coupon(note, *observation, value)
                                          : 0.0;
                const double continuation_in = next_knocked_in[index];
                const double continuation_out = transitioned ? continuation_in : next_alive[index];
                next_knocked_in[index] = continuation_in + coupon;
                next_alive[index] = continuation_out + coupon;
            } else if (transitioned) {
                next_alive[index] = next_knocked_in[index];
            }
        }
        knocked_in.swap(next_knocked_in);
        alive.swap(next_alive);
    }

    return PricingResult{
        {risk_measure::price,
         space->interpolate(note.touch_status() == barrier_touch_status::down ? knocked_in : alive,
                            spot)}};
}

template result<PricingResult> price_autocallable_finite_difference(
    const PhoenixOption&, const PricingContext&, FiniteDifferenceSettings);
template result<PricingResult> price_autocallable_finite_difference(
    const SnowballOption&, const PricingContext&, FiniteDifferenceSettings);
template result<PricingResult> price_autocallable_finite_difference(
    const BinarySnowballOption&, const PricingContext&, FiniteDifferenceSettings);
template result<PricingResult> price_autocallable_finite_difference(
    const TernarySnowballOption&, const PricingContext&, FiniteDifferenceSettings);

} // namespace kiyosi
