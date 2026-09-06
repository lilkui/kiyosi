#include <kiyosi/pricing/engines/barrier.hpp>
#include <kiyosi/pricing/engines/analytic.hpp>
#include "../detail/common.hpp"
#include <algorithm>
#include <cmath>
#include <vector>

namespace kiyosi {
using namespace detail;
namespace {
result<double> knockout_fd(const BarrierOption& option, const PricingContext& context, FiniteDifferenceSettings settings)
{
    auto valid = validate_expiry(context.valuation_date(), option.expiry());
    if (!valid) return std::unexpected(valid.error());
    if (option.observation() == observation_mode::scheduled) {
        auto schedule = validate_schedule(option.schedule(), context.valuation_date(), option.expiry(), context.calendar());
        if (!schedule) return std::unexpected(schedule.error());
    }
    const double maturity = actual_365(context.valuation_date(), option.expiry());
    const double spot = context.asset_price().value(), strike = option.strike();
    if (maturity == 0.0) return std::max((option.type() == option_type::call ? spot - strike : strike - spot), 0.0);
    const double rate = context.parameters().risk_free_rate(), dividend = context.parameters().dividend_yield(), volatility = context.parameters().volatility();
    const double barrier = option.barrier();
    const int asset_steps = settings.asset_steps;
    const double upper = settings.upper_boundary > 0.0 ? settings.upper_boundary : std::max(4.0 * strike, 4.0 * spot);
    if (upper <= std::max({spot, strike, barrier})) return std::unexpected(Error{error_category::invalid_parameter, "finite-difference upper boundary must exceed spot, strike, and barrier"});
    int time_steps = settings.time_steps;
    if (settings.scheme == finite_difference_scheme::explicit_euler) {
        const double required = maturity * volatility * volatility * asset_steps * asset_steps * 1.1;
        if (required > 100'000.0) return std::unexpected(Error{error_category::invalid_parameter, "explicit finite-difference grid requires too many time steps"});
        time_steps = std::max(time_steps, static_cast<int>(std::ceil(required)));
    }
    const double dt = maturity / time_steps, theta = settings.scheme == finite_difference_scheme::explicit_euler ? 0.0 : settings.scheme == finite_difference_scheme::implicit_euler ? 1.0 : 0.5, spacing = upper / asset_steps;
    const bool upper_barrier = option.barrier_kind() == barrier_type::up_and_in || option.barrier_kind() == barrier_type::up_and_out;
    std::vector<int> observation_steps;
    if (option.observation() == observation_mode::scheduled) for (auto value : option.observation_dates()) observation_steps.push_back(std::clamp(static_cast<int>(std::lround(actual_365(context.valuation_date(), value) / maturity * time_steps)), 1, time_steps));
    auto active = [&](int step) { return option.observation() == observation_mode::continuous || std::find(observation_steps.begin(), observation_steps.end(), step) != observation_steps.end(); };
    auto payoff = [&](double asset) { return std::max((option.type() == option_type::call ? asset - strike : strike - asset), 0.0); };
    auto knocked = [&](double asset) { return upper_barrier ? asset >= barrier : asset <= barrier; };
    auto rebate_value = [&](double tau) { return option.rebate_payment() == rebate_timing::at_hit ? option.rebate() : option.rebate() * std::exp(-rate * tau); };
    std::vector<double> old(asset_steps + 1), next(old.size());
    for (int index = 0; index <= asset_steps; ++index) old[index] = payoff(spacing * index);
    if (active(time_steps)) for (int index = 0; index <= asset_steps; ++index) if (knocked(spacing * index)) old[index] = option.rebate();
    std::vector<double> lower(asset_steps - 1), diagonal(lower.size()), upper_diagonal(lower.size()), rhs(lower.size());
    for (int step = time_steps - 1; step >= 0; --step) {
        const double tau = (time_steps - step) * dt;
        next.front() = option.type() == option_type::put ? strike * std::exp(-rate * tau) : 0.0;
        next.back() = option.type() == option_type::call ? upper * std::exp(-dividend * tau) - strike * std::exp(-rate * tau) : 0.0;
        for (int index = 1; index < asset_steps; ++index) {
            const double i = index, a = 0.5 * volatility * volatility * i * i - 0.5 * (rate - dividend) * i, b = -volatility * volatility * i * i - rate, c = 0.5 * volatility * volatility * i * i + 0.5 * (rate - dividend) * i;
            const auto position = static_cast<std::size_t>(index - 1);
            rhs[position] = old[index] + (1.0 - theta) * dt * (a * old[index - 1] + b * old[index] + c * old[index + 1]);
            if (index == 1) rhs[position] += theta * dt * a * next.front();
            if (index == asset_steps - 1) rhs[position] += theta * dt * c * next.back();
            lower[position] = -theta * dt * a; diagonal[position] = 1.0 - theta * dt * b; upper_diagonal[position] = -theta * dt * c;
        }
        if (theta == 0.0) std::copy(rhs.begin(), rhs.end(), next.begin() + 1);
        else {
            for (std::size_t i = 1; i < diagonal.size(); ++i) { const double factor = lower[i] / diagonal[i - 1]; diagonal[i] -= factor * upper_diagonal[i - 1]; rhs[i] -= factor * rhs[i - 1]; }
            if (diagonal.empty() || diagonal.back() == 0.0) return std::unexpected(Error{error_category::invalid_result, "finite-difference system is numerically unstable"});
            rhs.back() /= diagonal.back();
            for (std::size_t i = diagonal.size() - 1; i-- > 0;) rhs[i] = (rhs[i] - upper_diagonal[i] * rhs[i + 1]) / diagonal[i];
            std::copy(rhs.begin(), rhs.end(), next.begin() + 1);
        }
        if (active(step)) for (int index = 0; index <= asset_steps; ++index) if (knocked(spacing * index)) next[index] = rebate_value(tau);
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
    const bool knock_in = option.barrier_kind() == barrier_type::up_and_in || option.barrier_kind() == barrier_type::down_and_in;
    const bool touched = option.barrier_kind() == barrier_type::up_and_in || option.barrier_kind() == barrier_type::up_and_out
                             ? context.asset_price().value() >= option.barrier()
                             : context.asset_price().value() <= option.barrier();
    if (option.observation() == observation_mode::continuous && touched) {
        const double t = actual_365(context.valuation_date(), option.expiry());
        if (!knock_in) return PricingResult{{risk_measure::price, option.rebate_payment() == rebate_timing::at_hit ? option.rebate() : option.rebate() * std::exp(-context.parameters().risk_free_rate() * t)}};
        auto vanilla = price_at_volatility(*make_european_option(option.type(), option.strike(), option.expiry()), context, context.parameters().volatility(), risk_measure_output::price_only);
        if (!vanilla) return std::unexpected(vanilla.error());
        return PricingResult{{risk_measure::price, vanilla->get(risk_measure::price).value()}};
    }
    auto out = knockout_fd(option, context, settings_); if (!out) return std::unexpected(out.error());
    if (knock_in && touched) {
        auto vanilla = price_at_volatility(*make_european_option(option.type(), option.strike(), option.expiry()), context, context.parameters().volatility(), risk_measure_output::price_only);
        if (!vanilla) return std::unexpected(vanilla.error());
        return PricingResult{{risk_measure::price, vanilla->get(risk_measure::price).value()}};
    }
    if (!knock_in) return PricingResult{{risk_measure::price, *out}};
    auto vanilla = price_at_volatility(*make_european_option(option.type(), option.strike(), option.expiry()), context, context.parameters().volatility(), risk_measure_output::price_only);
    if (!vanilla) return std::unexpected(vanilla.error());
    const double t = actual_365(context.valuation_date(), option.expiry());
    return PricingResult{{risk_measure::price, vanilla->get(risk_measure::price).value() - *out + option.rebate() * std::exp(-context.parameters().risk_free_rate() * t)}};
}
} // namespace kiyosi
