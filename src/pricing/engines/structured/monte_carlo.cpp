#include <kiyosi/pricing/engines/structured/monte_carlo.hpp>

#include <cmath>
#include <optional>
#include <random>
#include <utility>
#include <vector>

#include <kiyosi/market/schedule.hpp>

#include "../../detail/autocallable_traits.hpp"
#include "../../detail/calendar_dates.hpp"
#include "../../detail/math.hpp"

namespace kiyosi {
using namespace detail;

namespace {

struct SimulationStep {
    date current;
    double drift;
    double diffusion;
    double discount;
};

struct SimulationInputs {
    std::vector<std::size_t> observation_schedule;
    std::vector<SimulationStep> steps;
    double terminal_discount;
};

struct PathState {
    double coupons{};
    bool knocked_in{};
    std::size_t observation_index{};
};

struct InitialState {
    PathState path;
    std::optional<double> settlement;
};

template <typename Note>
InitialState initial_state(const Note& note, const PricingContext& context,
                           const std::vector<std::size_t>& schedule)
{
    if (note.touch_status() == barrier_touch_status::up) return {{}, 0.0};

    const timestamp valuation = context.valuation_time();
    const double value = context.asset_price();
    PathState state{.knocked_in = note.touch_status() == barrier_touch_status::down};
    if (valuation == start_of_day(date_of(valuation)))
        state.knocked_in = is_knocked_in(note, value, state.knocked_in, valuation == note.expiry());

    if (!schedule.empty() && note.observation_dates()[schedule.front()] == valuation) {
        const auto event = schedule.front();
        const double coupon = observation_coupon(note, event, value);
        if (value >= note.knock_out_prices()[event])
            return {state, note.principal_ratio() + coupon};
        if constexpr (carries_observation_coupon<Note>) state.coupons = coupon;
        state.observation_index = 1;
    }
    if (valuation == note.expiry()) {
        state.knocked_in = is_knocked_in(note, value, state.knocked_in, true);
        return {state, state.coupons + terminal_settlement(note, value, state.knocked_in)};
    }
    return {state, std::nullopt};
}

template <typename Note>
SimulationInputs prepare_simulation(const Note& note, const PricingContext& context,
                                    std::vector<std::size_t> observation_schedule)
{
    const double rate = context.parameters().risk_free_rate();
    const double dividend = context.parameters().dividend_yield();
    const double sigma = context.parameters().volatility();
    const timestamp valuation = context.valuation_time();
    const auto dates = trading_dates(context.calendar(), valuation, note.expiry());
    std::vector<SimulationStep> steps;
    steps.reserve(dates.size());
    auto previous = valuation;
    for (const date current : dates) {
        const double dt = actual_365(previous, current);
        steps.push_back({current,
                         (rate - dividend - 0.5 * sigma * sigma) * dt,
                         sigma * std::sqrt(dt),
                         std::exp(-rate * actual_365(valuation, current))});
        previous = current;
    }
    return {std::move(observation_schedule), std::move(steps),
            std::exp(-rate * actual_365(valuation, note.expiry()))};
}

template <typename Note>
double path_payoff(const Note& note, const PricingContext& context,
                   const SimulationInputs& inputs, PathState state,
                   std::mt19937_64& generator)
{
    const auto& dates = note.observation_dates();
    const auto& schedule = inputs.observation_schedule;

    double value = context.asset_price();
    std::normal_distribution<double> normal;
    for (const auto& step : inputs.steps) {
        value *= std::exp(step.drift + step.diffusion * normal(generator));
        state.knocked_in = is_knocked_in(note, value, state.knocked_in, false);
        if (state.observation_index >= schedule.size() ||
            dates[schedule[state.observation_index]] != step.current)
            continue;
        const auto event = schedule[state.observation_index];
        const double coupon = observation_coupon(note, event, value);
        if (value >= note.knock_out_prices()[event])
            return (note.principal_ratio() + coupon) * step.discount + state.coupons;
        if constexpr (carries_observation_coupon<Note>) state.coupons += coupon * step.discount;
        ++state.observation_index;
    }
    state.knocked_in = is_knocked_in(note, value, state.knocked_in, true);
    return state.coupons +
           inputs.terminal_discount * terminal_settlement(note, value, state.knocked_in);
}

} // namespace

template <typename Note>
result<PricingResult> MonteCarloStructuredEngine<Note>::price(
    const Note& note, const PricingContext& context) const
{
    auto contract = validate_note(note);
    if (!contract) return std::unexpected(contract.error());
    auto valid = validate_life(context.valuation_time(), note.effective(), note.expiry());
    if (!valid) return std::unexpected(valid.error());
    auto schedule = validate_observation_dates(note.observation_dates(), note.effective(),
                                               note.expiry(), context.calendar());
    if (!schedule) return std::unexpected(schedule.error());
    if (settings_.path_count <= 0 || settings_.path_count > 10'000'000)
        return std::unexpected(Error{error_category::invalid_parameter,
                                     "structured Monte Carlo path count is out of range"});

    const auto make_result = [](double value) -> result<PricingResult> {
        if (!std::isfinite(value))
            return std::unexpected(Error{error_category::invalid_result,
                                         "structured pricing produced a non-finite result"});
        return make_pricing_result({{risk_measure::price, value}});
    };
    auto observation_indices = observation_schedule(note, context.valuation_time());
    const auto initial = initial_state(note, context, observation_indices);
    if (initial.settlement) return make_result(*initial.settlement);

    const auto inputs = prepare_simulation(note, context, std::move(observation_indices));
    std::mt19937_64 generator(settings_.seed ? *settings_.seed : std::random_device{}());
    double sum = 0.0;
    for (int path = 0; path < settings_.path_count; ++path)
        sum += path_payoff(note, context, inputs, initial.path, generator);
    return make_result(sum / static_cast<double>(settings_.path_count));
}

template class MonteCarloStructuredEngine<PhoenixOption>;
template class MonteCarloStructuredEngine<SnowballOption>;
template class MonteCarloStructuredEngine<BinarySnowballOption>;
template class MonteCarloStructuredEngine<TernarySnowballOption>;

} // namespace kiyosi
