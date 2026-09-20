#include <kiyosi/pricing/engines/structured/monte_carlo.hpp>

#include <cmath>
#include <cstddef>
#include <new>
#include <optional>
#include <random>
#include <utility>
#include <vector>

#include <kiyosi/market/schedule.hpp>

#include "../../detail/autocallable_traits.hpp"
#include "../../detail/calendar_dates.hpp"
#include "../../detail/math.hpp"
#include "../monte_carlo_cuda.hpp"

namespace kiyosi {
using namespace detail;

namespace {

struct SimulationInputs {
    std::vector<CudaStructuredStep> step_count;
    double terminal_discount;
};

struct InitialState {
    AutocallablePathState path{};
    std::size_t next_observation{};
    std::optional<double> settlement;
};

template <typename Note>
InitialState initial_state(const Note& note, const PricingContext& context,
                           const AutocallableProgram& program,
                           const std::vector<std::size_t>& schedule)
{
    if (note.barrier_state() == AutocallableBarrierState::knocked_out)
        return {.settlement = 0.0};

    const Timestamp valuation = context.valuation_time();
    const double value = context.spot_price();
    InitialState initial{
        .path = {.coupons = 0.0,
                 .knocked_in = note.barrier_state() == AutocallableBarrierState::knocked_in}};
    if (valuation == start_of_day(date_of(valuation)))
        initial.path.knocked_in = program_knocked_in(
            program, value, initial.path.knocked_in, valuation == note.expiry_date());

    if (!schedule.empty() && note.observation_dates()[schedule.front()] == valuation) {
        const auto event = autocallable_event(note, schedule.front());
        const double coupon = program_observation_coupon(event, value);
        if (value >= event.knock_out_level)
            return {.path = initial.path,
                    .next_observation = 1,
                    .settlement = program.principal_ratio + coupon};
        if (program.carries_observation_coupon) initial.path.coupons = coupon;
        initial.next_observation = 1;
    }
    if (valuation == note.expiry_date()) {
        initial.path.knocked_in =
            program_knocked_in(program, value, initial.path.knocked_in, true);
        initial.settlement = initial.path.coupons +
                             program_terminal_settlement(
                                 program, value, initial.path.knocked_in);
    }
    return initial;
}

template <typename Note>
SimulationInputs prepare_simulation(
    const Note& note, const PricingContext& context,
    const std::vector<std::size_t>& remaining_observation_indices, std::size_t next_observation)
{
    const double rate = context.model_parameters().risk_free_rate();
    const double dividend = context.model_parameters().dividend_yield();
    const double sigma = context.model_parameters().volatility();
    const Timestamp valuation = context.valuation_time();
    const auto dates = trading_dates(context.calendar(), valuation, note.expiry_date());
    std::vector<CudaStructuredStep> steps;
    steps.reserve(dates.size());
    auto previous = valuation;
    for (const Date current : dates) {
        const double dt = actual_365_fixed_year_fraction(previous, current);
        AutocallableEvent event{};
        if (next_observation < remaining_observation_indices.size() &&
            note.observation_dates()[remaining_observation_indices[next_observation]] == current) {
            event = autocallable_event(note, remaining_observation_indices[next_observation]);
            ++next_observation;
        }
        steps.push_back({{(rate - dividend - 0.5 * sigma * sigma) * dt,
                          sigma * std::sqrt(dt),
                          std::exp(-rate * actual_365_fixed_year_fraction(valuation, current))},
                         event});
        previous = current;
    }
    return {std::move(steps),
            std::exp(-rate * actual_365_fixed_year_fraction(valuation, note.expiry_date()))};
}

double path_payoff(double initial_spot, const AutocallableProgram& program,
                   const SimulationInputs& inputs, AutocallablePathState state,
                   std::mt19937_64& generator)
{
    double value = initial_spot;
    std::normal_distribution<double> normal;
    for (const auto& step : inputs.step_count) {
        value *= std::exp(step.simulation.drift +
                          step.simulation.diffusion * normal(generator));
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

#if KIYOSI_HAS_CUDA
std::uint64_t random_seed()
{
    std::random_device source;
    return (static_cast<std::uint64_t>(source()) << 32U) ^
           static_cast<std::uint64_t>(source());
}

Result<double> cuda_sum(CudaPricingResult cuda_result)
{
    switch (cuda_result.status) {
    case CudaPricingStatus::success:
        return cuda_result.payoff_sum;
    case CudaPricingStatus::unavailable:
        return std::unexpected(Error{ErrorCategory::backend_unavailable, cuda_result.message});
    case CudaPricingStatus::failure:
        return std::unexpected(Error{ErrorCategory::backend_failure, cuda_result.message});
    case CudaPricingStatus::out_of_memory:
        throw std::bad_alloc{};
    case CudaPricingStatus::invalid_result:
        return std::unexpected(Error{ErrorCategory::invalid_result, cuda_result.message});
    }
    return std::unexpected(Error{ErrorCategory::backend_failure,
                                 "CUDA Monte Carlo returned an unknown status"});
}
#endif

} // namespace

template <typename Note>
Result<PricingResult> MonteCarloAutocallableEngine<Note>::price(
    const Note& note, const PricingContext& context) const
{
    auto note_validation = validate_autocallable_note(note);
    if (!note_validation) return std::unexpected(note_validation.error());
    auto valid = validate_valuation_within_instrument_life(context.valuation_time(), note.effective_date(), note.expiry_date());
    if (!valid) return std::unexpected(valid.error());
    auto schedule = validate_observation_dates(note.observation_dates(), note.effective_date(),
                                               note.expiry_date(), context.calendar());
    if (!schedule) return std::unexpected(schedule.error());
    if (settings_.path_count <= 0 || settings_.path_count > 10'000'000)
        return std::unexpected(Error{ErrorCategory::invalid_parameter,
                                     "structured Monte Carlo path count is out of range"});
    if (settings_.backend != MonteCarloBackend::cpu &&
        settings_.backend != MonteCarloBackend::cuda)
        return std::unexpected(Error{ErrorCategory::invalid_parameter,
                                     "Monte Carlo backend is invalid"});

    const auto make_result = [](double value) -> Result<PricingResult> {
        if (!std::isfinite(value))
            return std::unexpected(Error{ErrorCategory::invalid_result,
                                         "structured pricing produced a non-finite result"});
        return make_pricing_result({{RiskMeasure::price, value}});
    };
    const auto program = autocallable_program(note);
    const auto observation_indices = remaining_observation_indices(note, context.valuation_time());
    const auto initial = initial_state(note, context, program, observation_indices);
    if (initial.settlement) return make_result(*initial.settlement);

    const auto inputs = prepare_simulation(
        note, context, observation_indices, initial.next_observation);
    if (settings_.backend == MonteCarloBackend::cuda) {
#if KIYOSI_HAS_CUDA
        const auto sum = cuda_sum(cuda_structured_price(
            {settings_.path_count, settings_.seed ? *settings_.seed : random_seed(),
             context.spot_price(), inputs.terminal_discount, program, initial.path},
            inputs.step_count));
        if (!sum) return std::unexpected(sum.error());
        return make_result(*sum / static_cast<double>(settings_.path_count));
#else
        return std::unexpected(Error{ErrorCategory::backend_unavailable,
                                     "CUDA support is not enabled in this build"});
#endif
    }

    std::mt19937_64 generator(settings_.seed ? *settings_.seed : std::random_device{}());
    double sum = 0.0;
    for (int path = 0; path < settings_.path_count; ++path)
        sum += path_payoff(context.spot_price(), program, inputs, initial.path, generator);
    return make_result(sum / static_cast<double>(settings_.path_count));
}

template class MonteCarloAutocallableEngine<PhoenixOption>;
template class MonteCarloAutocallableEngine<SnowballOption>;
template class MonteCarloAutocallableEngine<BinarySnowballOption>;
template class MonteCarloAutocallableEngine<TernarySnowballOption>;

} // namespace kiyosi
