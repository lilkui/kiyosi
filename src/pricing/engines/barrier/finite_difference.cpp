#include <kiyosi/pricing/engines/barrier/finite_difference.hpp>
#include <kiyosi/pricing/engines/vanilla/analytic.hpp>
#include "../../detail/common.hpp"
#include "../../detail/finite_difference.hpp"
#include <algorithm>
#include <cmath>
#include <vector>

namespace kiyosi {
using namespace detail;
namespace {
result<double> knockout_fd(const BarrierOption& option, const PricingContext& context, FiniteDifferenceSettings settings)
{
    auto valid = validate_life(context.valuation_time(), option.effective(), option.expiry());
    if (!valid) return std::unexpected(valid.error());
    if (option.observation() == observation_mode::scheduled) {
        auto schedule = validate_schedule(option.schedule(), option.effective(), option.expiry(), context.calendar());
        if (!schedule) return std::unexpected(schedule.error());
    }
    const double maturity = actual_365(context.valuation_time(), option.expiry());
    const double spot = context.asset_price().value(), strike = option.strike();
    if (maturity == 0.0) return std::max((option.type() == option_type::call ? spot - strike : strike - spot), 0.0);
    const double rate = context.parameters().risk_free_rate(), dividend = context.parameters().dividend_yield(), volatility = context.parameters().volatility();
    const double barrier = option.barrier();
    const int asset_steps = settings.asset_steps;
    const double upper = settings.upper_boundary > 0.0 ? settings.upper_boundary : std::max(4.0 * strike, 4.0 * spot);
    if (upper <= std::max({spot, strike, barrier})) return std::unexpected(Error{error_category::invalid_parameter, "finite-difference upper boundary must exceed spot, strike, and barrier"});
    const int time_steps = settings.time_steps;
    const double theta = settings.scheme == finite_difference_scheme::explicit_euler ? 0.0 : settings.scheme == finite_difference_scheme::implicit_euler ? 1.0 : 0.5, spacing = upper / asset_steps;
    const bool upper_barrier = option.barrier_kind() == barrier_type::up_and_in || option.barrier_kind() == barrier_type::up_and_out;
    std::vector<double> observation_times;
    if (option.observation() == observation_mode::scheduled) {
        for (auto value : option.observation_dates()) {
            if (value < context.valuation_time()) continue;
            const double event_time = actual_365(context.valuation_time(), value);
            if (event_time >= 0.0 && event_time <= maturity) observation_times.push_back(event_time);
        }
    }
    const auto grid = finite_difference_grid(maturity, time_steps, observation_times);
    if (settings.scheme == finite_difference_scheme::explicit_euler) {
        double max_dt = 0.0;
        for (std::size_t index = 1; index < grid.size(); ++index)
            max_dt = std::max(max_dt, grid[index] - grid[index - 1]);
        if (max_dt * (volatility * volatility * asset_steps * asset_steps + rate) > 1.0)
            return std::unexpected(Error{error_category::invalid_parameter, "explicit finite-difference grid is unstable"});
    }
    auto active = [&](double time) { return option.observation() == observation_mode::continuous || std::binary_search(observation_times.begin(), observation_times.end(), time); };
    std::sort(observation_times.begin(), observation_times.end());
    auto payoff = [&](double asset) { return std::max((option.type() == option_type::call ? asset - strike : strike - asset), 0.0); };
    auto knocked = [&](double asset) { return upper_barrier ? asset >= barrier : asset <= barrier; };
    auto rebate_value = [&](double tau) { return option.rebate_payment() == rebate_timing::at_hit ? option.rebate() : option.rebate() * std::exp(-rate * tau); };
    std::vector<double> old(asset_steps + 1), next(old.size());
    for (int index = 0; index <= asset_steps; ++index) old[index] = payoff(spacing * index);
    if (active(maturity)) for (int index = 0; index <= asset_steps; ++index) if (knocked(spacing * index)) old[index] = option.rebate();
    FiniteDifferenceStep stepper(old.size());
    for (int step = static_cast<int>(grid.size()) - 2; step >= 0; --step) {
        const double dt = grid[step + 1] - grid[step];
        const double tau = maturity - grid[step];
        next.front() = option.type() == option_type::put ? strike * std::exp(-rate * tau) : 0.0;
        next.back() = option.type() == option_type::call ? upper * std::exp(-dividend * tau) - strike * std::exp(-rate * tau) : 0.0;
        if (option.observation() == observation_mode::continuous) {
            if (knocked(0.0)) next.front() = rebate_value(tau);
            if (knocked(upper)) next.back() = rebate_value(tau);
        }
        const auto constraint = [&](int index) -> std::optional<double> {
            if (option.observation() == observation_mode::continuous && knocked(spacing * index))
                return rebate_value(tau);
            return std::nullopt;
        };
        if (!stepper.advance(old, next, dt, rate, dividend, volatility, theta,
                             next.front(), next.back(), constraint))
            return std::unexpected(Error{error_category::invalid_result,
                                         "finite-difference system is numerically unstable"});
        if (active(grid[step])) for (int index = 0; index <= asset_steps; ++index) if (knocked(spacing * index)) next[index] = rebate_value(tau);
        old.swap(next);
    }
    const double position = spot / spacing; const int index = std::clamp(static_cast<int>(std::floor(position)), 0, asset_steps - 1);
    return old[index] + (position - index) * (old[index + 1] - old[index]);
}
}
result<PricingResult> FiniteDifferenceBarrierEngine::price(const BarrierOption& option, const PricingContext& context) const
{
    if (settings_.asset_steps < 3 || settings_.asset_steps > 10'000 || settings_.time_steps <= 0 || settings_.time_steps > 100'000) return std::unexpected(Error{error_category::invalid_parameter, "finite-difference grid dimensions are out of range"});
    if (settings_.upper_boundary != 0.0 && (!std::isfinite(settings_.upper_boundary) || settings_.upper_boundary <= 0.0)) return std::unexpected(Error{error_category::invalid_parameter, "finite-difference upper boundary must be finite and positive"});
    if (settings_.scheme != finite_difference_scheme::explicit_euler && settings_.scheme != finite_difference_scheme::implicit_euler && settings_.scheme != finite_difference_scheme::crank_nicolson) return std::unexpected(Error{error_category::invalid_parameter, "finite-difference scheme is invalid"});
    auto valid = validate_life(context.valuation_time(), option.effective(), option.expiry());
    if (!valid) return std::unexpected(valid.error());
    if (option.observation() == observation_mode::scheduled) {
        auto schedule = validate_schedule(option.schedule(), option.effective(), option.expiry(), context.calendar());
        if (!schedule) return std::unexpected(schedule.error());
    }
    const bool knock_in = option.barrier_kind() == barrier_type::up_and_in || option.barrier_kind() == barrier_type::down_and_in;
    const bool touched = option.barrier_kind() == barrier_type::up_and_in || option.barrier_kind() == barrier_type::up_and_out
                             ? context.asset_price().value() >= option.barrier()
                             : context.asset_price().value() <= option.barrier();
    const bool observed_now = option.observation() == observation_mode::continuous ||
        std::ranges::any_of(option.observation_dates(), [&](date event) {
            return event == context.valuation_time();
        });
    if (touched && observed_now) {
        const double t = actual_365(context.valuation_time(), option.expiry());
        if (!knock_in) return PricingResult{{risk_measure::price, option.rebate_payment() == rebate_timing::at_hit ? option.rebate() : option.rebate() * std::exp(-context.parameters().risk_free_rate() * t)}};
        auto vanilla = price_at_volatility(*make_european_option(option.type(), option.strike(), option.effective(), option.expiry()), context, context.parameters().volatility(), risk_measure_output::price_only);
        if (!vanilla) return std::unexpected(vanilla.error());
        return PricingResult{{risk_measure::price, vanilla->get(risk_measure::price).value()}};
    }
    auto out = knockout_fd(option, context, settings_); if (!out) return std::unexpected(out.error());
    if (knock_in && touched && observed_now) {
        auto vanilla = price_at_volatility(*make_european_option(option.type(), option.strike(), option.effective(), option.expiry()), context, context.parameters().volatility(), risk_measure_output::price_only);
        if (!vanilla) return std::unexpected(vanilla.error());
        return PricingResult{{risk_measure::price, vanilla->get(risk_measure::price).value()}};
    }
    if (!knock_in) return PricingResult{{risk_measure::price, *out}};
    auto vanilla = price_at_volatility(*make_european_option(option.type(), option.strike(), option.effective(), option.expiry()), context, context.parameters().volatility(), risk_measure_output::price_only);
    if (!vanilla) return std::unexpected(vanilla.error());
    const double t = actual_365(context.valuation_time(), option.expiry());
    return PricingResult{{risk_measure::price, vanilla->get(risk_measure::price).value() - *out + option.rebate() * std::exp(-context.parameters().risk_free_rate() * t)}};
}
} // namespace kiyosi
