#include <kiyosi/pricing/engines/autocallable/monte_carlo.hpp>

#include <kiyosi/pricing/numerical_greeks.hpp>

#include <algorithm>
#include <cmath>
#include <cstddef>
#include <optional>
#include <random>
#include <utility>
#include <vector>

#include <kiyosi/market/schedule.hpp>
#include <kiyosi/pricing/detail/autocallable_traits.hpp>

#include "../../detail/calendar_dates.hpp"
#include "../../detail/math.hpp"
#include "../monte_carlo_host.hpp"

namespace kiyosi {
using namespace detail;

namespace {

struct SimulationInputs {
    std::vector<CudaAutocallableStep> step_count;
    double terminal_discount;
};

template <typename Note>
Result<SimulationInputs> prepare_simulation(
    const Note& note, const PricingContext& context, std::size_t next_observation, bool daily_knock_in)
{
    const double rate = context.model_parameters().risk_free_rate();
    const Timestamp valuation = context.valuation_time();
    const auto dates = [&] {
        if (daily_knock_in) return trading_dates(context.calendar(), valuation, note.expiry_date());
        std::vector<Date> events(note.observation_dates().begin() + next_observation, note.observation_dates().end());
        if (events.empty() || events.back() != note.expiry_date()) events.push_back(note.expiry_date());
        return events;
    }();
    std::vector<CudaAutocallableStep> steps;
    steps.reserve(dates.size());
    auto previous = valuation;
    for (const Date current : dates) {
        AutocallableEvent event{};
        if (next_observation < note.observation_dates().size() &&
            note.observation_dates()[next_observation] == current) {
            event = autocallable_event(note, next_observation);
            ++next_observation;
        }
        const auto step = simulation_step(context, previous, current);
        if (!step) return std::unexpected(step.error());
        steps.push_back({*step, event});
        previous = current;
    }
    return SimulationInputs{std::move(steps),
                            std::exp(-rate * actual_365_fixed_year_fraction(valuation, note.expiry_date()))};
}

Result<double> path_payoff(double initial_spot, const AutocallableProgram& program,
                           const SimulationInputs& inputs, AutocallablePathState state,
                           Pcg32& generator)
{
    double value = initial_spot;
    std::normal_distribution<double> normal;
    for (const auto& step : inputs.step_count) {
        value *= std::exp(step.simulation.drift +
                          step.simulation.diffusion * normal(generator));
        if (!std::isfinite(value) || value <= 0.0)
            return std::unexpected(Error{ErrorCategory::invalid_result,
                                         "Monte Carlo simulation produced an invalid asset price"});
        state.knocked_in = program_knocked_in(program, value, state.knocked_in, false);
        if (!step.event.active) continue;
        const double coupon = program_observation_coupon(step.event, value);
        if (value >= step.event.knock_out_level)
            return (program.principal_ratio + coupon) * step.simulation.discount +
                   state.coupons;
        if (program.carries_observation_coupon)
            state.coupons += coupon * step.simulation.discount;
    }
    state.knocked_in = program_knocked_in(program, value, state.knocked_in, true);
    return state.coupons + inputs.terminal_discount *
                               program_terminal_settlement(program, value, state.knocked_in);
}

} // namespace

template <typename Note>
Result<double> MonteCarloAutocallableEngine<Note>::price(
    const Note& note, const PricingContext& context) const
{
    auto note_validation = validate_autocallable_note(note);
    if (!note_validation) return std::unexpected(note_validation.error());
    auto valid = validate_valuation_within_instrument_life(context.valuation_time(), note.effective_date(), note.expiry_date());
    if (!valid) return std::unexpected(valid.error());
    auto schedule = validate_observation_trading_days(note.observation_dates(), context.calendar());
    if (!schedule) return std::unexpected(schedule.error());
    auto history = validate_autocallable_history(note, context);
    if (!history) return std::unexpected(history.error());
    const auto settings_valid = validate_monte_carlo_settings(settings_);
    if (!settings_valid) return std::unexpected(settings_valid.error());
    auto expiry_valid = validate_trading_expiry(context.calendar(), note.expiry_date());
    if (!expiry_valid) return std::unexpected(expiry_valid.error());

    const auto program = autocallable_program(note);
    const auto initial = autocallable_initial_state(note, context, program);
    if (initial.settlement) return checked_price(*initial.settlement);

    const auto inputs = prepare_simulation(note, context, initial.next_observation,
                                           program.has_knock_in && program.daily_knock_in && !initial.path.knocked_in);
    if (!inputs) return std::unexpected(inputs.error());
    if (settings_.backend == MonteCarloBackend::cuda) {
#if KIYOSI_HAS_CUDA
        const auto mean = cuda_mean(cuda_autocallable_price(
            {settings_.path_count, settings_.seed ? *settings_.seed : random_seed(),
             context.spot_price(), inputs->terminal_discount, program, initial.path},
            inputs->step_count));
        if (!mean) return std::unexpected(mean.error());
        return checked_price(*mean);
#else
        return std::unexpected(Error{ErrorCategory::backend_unavailable,
                                     "CUDA support is not enabled in this build"});
#endif
    }

    return cpu_path_mean(settings_, [&](auto& generator) {
        return path_payoff(context.spot_price(), program, *inputs, initial.path, generator);
    });
}

template <typename Note>
Result<PricingResult> MonteCarloAutocallableEngine<Note>::price_with_greeks(const Note& option, const PricingContext& context,
                                                                            GreeksRequest greeks, NumericalShiftSettings settings) const
{
    return detail::price_with_greeks(*this, option, context, greeks, settings);
}

template class MonteCarloAutocallableEngine<PhoenixOption>;
template class MonteCarloAutocallableEngine<SnowballOption>;
template class MonteCarloAutocallableEngine<BinarySnowballOption>;
template class MonteCarloAutocallableEngine<TernarySnowballOption>;

} // namespace kiyosi
