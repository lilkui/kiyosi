#include <kiyosi/pricing/engines/structured/monte_carlo.hpp>

#include <algorithm>
#include <cmath>
#include <cstddef>
#include <optional>
#include <random>
#include <utility>
#include <vector>

#include <kiyosi/market/schedule.hpp>

#include "../../detail/autocallable_traits.hpp"
#include "../../detail/calendar_dates.hpp"
#include "../../detail/math.hpp"
#include "../monte_carlo_cuda_host.hpp"

namespace kiyosi {
using namespace detail;

namespace {

struct SimulationInputs {
    std::vector<CudaStructuredStep> step_count;
    double terminal_discount;
};

template <typename Note>
Result<SimulationInputs> prepare_simulation(
    const Note& note, const PricingContext& context, std::size_t next_observation)
{
    const double rate = context.model_parameters().risk_free_rate();
    const Timestamp valuation = context.valuation_time();
    const auto dates = trading_dates(context.calendar(), valuation, note.expiry_date());
    std::vector<CudaStructuredStep> steps;
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
                   std::mt19937_64& generator)
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
Result<PricingResult> MonteCarloAutocallableEngine<Note>::price_native(
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
    if (initial.settlement) return make_pricing_result(*initial.settlement);

    const auto inputs = prepare_simulation(note, context, initial.next_observation);
    if (!inputs) return std::unexpected(inputs.error());
    if (settings_.backend == MonteCarloBackend::cuda) {
#if KIYOSI_HAS_CUDA
        const auto sum = cuda_sum(cuda_structured_price(
            {settings_.path_count, settings_.seed ? *settings_.seed : random_seed(),
             context.spot_price(), inputs->terminal_discount, program, initial.path},
            inputs->step_count));
        if (!sum) return std::unexpected(sum.error());
        return make_pricing_result(*sum / static_cast<double>(settings_.path_count));
#else
        return std::unexpected(Error{ErrorCategory::backend_unavailable,
                                     "CUDA support is not enabled in this build"});
#endif
    }

    std::mt19937_64 generator(settings_.seed ? *settings_.seed : std::random_device{}());
    double sum = 0.0;
    for (int path = 0; path < settings_.path_count; ++path) {
        const auto payoff = path_payoff(context.spot_price(), program, *inputs, initial.path, generator);
        if (!payoff) return std::unexpected(payoff.error());
        sum += *payoff;
    }
    return make_pricing_result(sum / static_cast<double>(settings_.path_count));
}

template class MonteCarloAutocallableEngine<PhoenixOption>;
template class MonteCarloAutocallableEngine<SnowballOption>;
template class MonteCarloAutocallableEngine<BinarySnowballOption>;
template class MonteCarloAutocallableEngine<TernarySnowballOption>;

} // namespace kiyosi
